//! OCSP: the responses of the test responder (`certs/gen_ocsp.sh`), the
//! queries over HTTP to a responder in this process, and the identities
//! and handshakes that use them.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustorb_dds_security::auth::{Handshake, Identity, IdentityOptions};
use rustorb_dds_security::ocsp::{self, CertStatus, OcspOptions, OcspResponse};
use rustorb_dds_security::pki::Cert;

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{D}{f}")).unwrap()
}

fn cert(f: &str) -> Cert {
    Cert::parse(&read(f)).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Serial numbers and the responses about them.
type Answers = Arc<Mutex<Vec<(Vec<u8>, Vec<u8>)>>>;

/// An OCSP responder over HTTP in this process: it answers each request
/// with the response file of the certificate whose serial number is in
/// it, and 404 for others. The answers can be changed while it runs.
struct Responder {
    url: String,
    answers: Answers,
    queries: Arc<AtomicUsize>,
}

impl Responder {
    fn new(answers: &[(&str, &str)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/ocsp", listener.local_addr().unwrap());
        let r = Responder {
            url,
            answers: Arc::new(Mutex::new(Vec::new())),
            queries: Arc::new(AtomicUsize::new(0)),
        };
        for (c, f) in answers {
            r.answer(c, f);
        }
        let answers = r.answers.clone();
        let queries = r.queries.clone();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { continue };
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                // The head, then as much body as it says.
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&buf[..n]);
                    if let Some(end) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..end]).to_lowercase();
                        let len: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        if req.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                queries.fetch_add(1, Ordering::SeqCst);
                let found = answers
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(serial, _)| req.windows(serial.len()).any(|w| w == serial))
                    .map(|(_, a)| a.clone());
                let _ = match found {
                    Some(a) => s
                        .write_all(
                            format!(
                                "HTTP/1.0 200 OK\r\nContent-Type: application/ocsp-response\r\nContent-Length: {}\r\n\r\n",
                                a.len()
                            )
                            .as_bytes(),
                        )
                        .and_then(|_| s.write_all(&a)),
                    None => s.write_all(b"HTTP/1.0 404 Not Found\r\n\r\n"),
                };
            }
        });
        r
    }

    /// Answers about the certificate file `c` with the response file `f`.
    fn answer(&self, c: &str, f: &str) {
        let serial = cert(c).issuer_and_serial().1;
        let mut a = self.answers.lock().unwrap();
        a.retain(|(s, _)| *s != serial);
        a.push((serial, read(f)));
    }

    fn options(&self) -> OcspOptions {
        OcspOptions {
            responder: Some(self.url.clone()),
            timeout: Duration::from_secs(5),
            refresh: Duration::from_secs(300),
        }
    }
}

fn identity(who: &str, ocsp: Option<OcspOptions>) -> Result<Identity, rustorb_dds_security::Error> {
    Identity::with_options(
        &read("identity_ca.pem"),
        &read(&format!("{who}_cert.pem")),
        &read(&format!("{who}_key.pem")),
        &IdentityOptions {
            ocsp,
            ..IdentityOptions::default()
        },
    )
}

#[test]
fn responses_are_checked() {
    let ca = cert("identity_ca.pem");
    let (p1, p2, p3) = (
        cert("p1_cert.pem"),
        cert("p2_cert.pem"),
        cert("p3_cert.pem"),
    );
    let check = |f: &str, c: &Cert| OcspResponse::parse(&read(f)).unwrap().check(c, &ca, now());
    let good = check("ocsp_p1_good.der", &p1).unwrap();
    assert_eq!(good.status, CertStatus::Good);
    assert!(good.next_update.is_some_and(|n| n > now()));
    // A CertID of SHA-256.
    assert_eq!(
        check("ocsp_p1_good_sha256.der", &p1).unwrap().status,
        CertStatus::Good
    );
    // Signed by a responder the CA delegated to.
    assert_eq!(
        check("ocsp_p2_good_delegated.der", &p2).unwrap().status,
        CertStatus::Good
    );
    assert_eq!(
        check("ocsp_p3_revoked.der", &p3).unwrap().status,
        CertStatus::Revoked
    );
    // Signed by another CA; about another certificate; expired.
    let e = check("ocsp_p1_wrong_signer.der", &p1).unwrap_err();
    assert!(e.to_string().contains("not signed"), "{e}");
    let e = check("ocsp_p1_good.der", &p2).unwrap_err();
    assert!(e.to_string().contains("not about"), "{e}");
    let r = OcspResponse::parse(&read("ocsp_p1_good.der")).unwrap();
    assert!(r.check(&p1, &ca, 200 * 365 * 86400).is_err());
    assert!(OcspResponse::parse(b"not DER").is_err());
}

#[test]
fn queries_over_http() {
    let r = Responder::new(&[
        ("p1_cert.pem", "ocsp_p1_good.der"),
        ("p3_cert.pem", "ocsp_p3_revoked.der"),
    ]);
    let ca = cert("identity_ca.pem");
    let t = Duration::from_secs(5);
    for (c, want) in [
        ("p1_cert.pem", CertStatus::Good),
        ("p3_cert.pem", CertStatus::Revoked),
    ] {
        let c = cert(c);
        let resp = ocsp::query(&r.url, &c, &ca, t).unwrap();
        assert_eq!(resp.check(&c, &ca, now()).unwrap().status, want);
    }
    let e = ocsp::query(&r.url, &cert("p2_cert.pem"), &ca, t).unwrap_err();
    assert!(e.to_string().contains("404"), "{e}");
    assert!(ocsp::query("https://localhost/", &cert("p1_cert.pem"), &ca, t).is_err());
}

#[test]
fn identities_with_ocsp() {
    let r = Responder::new(&[
        ("p1_cert.pem", "ocsp_p1_good.der"),
        ("p3_cert.pem", "ocsp_p3_revoked.der"),
    ]);
    let p1 = identity("p1", Some(r.options())).unwrap();
    assert_eq!(p1.stapled_ocsp().unwrap(), read("ocsp_p1_good.der"));
    // A revoked identity, or one the responder does not know, fails.
    let e = identity("p3", Some(r.options())).unwrap_err();
    assert!(e.to_string().contains("revoked"), "{e}");
    assert!(identity("p2", Some(r.options())).is_err());

    // Peers: asked of the responder, once while the status holds.
    let p3_pem = read("p3_cert.pem");
    let e = p1.verify_peer(&p3_pem).unwrap_err();
    assert!(e.to_string().contains("revoked"), "{e}");
    let n = r.queries.load(Ordering::SeqCst);
    assert!(p1.verify_peer(&p3_pem).is_err());
    assert_eq!(r.queries.load(Ordering::SeqCst), n);
    // A stapled good response is proof enough: no query.
    let p2_pem = read("p2_cert.pem");
    p1.verify_peer_with(&p2_pem, Some(&read("ocsp_p2_good_delegated.der")))
        .unwrap();
    assert_eq!(r.queries.load(Ordering::SeqCst), n);
    // Without a response the responder knows nothing of participant2.
    assert!(
        identity("p1", Some(r.options()))
            .unwrap()
            .verify_peer(&p2_pem)
            .is_err()
    );

    // Without OCSP: stapled responses still count; a bad one is ignored.
    let plain = identity("p1", None).unwrap();
    assert!(plain.stapled_ocsp().is_none());
    assert!(plain.ocsp_status(&cert("p3_cert.pem")).is_none());
    assert!(
        plain
            .verify_peer_with(&p3_pem, Some(&read("ocsp_p3_revoked.der")))
            .is_err()
    );
    plain.verify_peer_with(&p3_pem, Some(b"garbage")).unwrap();
    plain
        .verify_peer_with(&p2_pem, Some(&read("ocsp_p1_good.der")))
        .unwrap();
}

/// A ParticipantBuiltinTopicData of PL_CDR_BE with only the GUID.
fn pdata(prefix: [u8; 12]) -> Vec<u8> {
    let mut b = vec![0, 2, 0, 0, 0x00, 0x50, 0, 16];
    b.extend_from_slice(&prefix);
    b.extend_from_slice(&[0, 0, 1, 0xc1, 0, 1, 0, 0]);
    b
}

#[test]
fn handshakes_staple_the_status() {
    let r = Responder::new(&[
        ("p1_cert.pem", "ocsp_p1_good.der"),
        ("p3_cert.pem", "ocsp_p3_revoked.der"),
    ]);
    // participant2 has no OCSP of its own, but takes participant1's
    // stapled response.
    let a = identity("p1", Some(r.options())).unwrap();
    let b = identity("p2", None).unwrap();
    let pa = a.adjusted_prefix(&[1; 16]);
    let pb = b.adjusted_prefix(&[2; 16]);
    let mut req = Handshake::request(&a, b"perm-a", &pdata(pa), None);
    assert_eq!(
        req.sent.binary("ocsp_status").unwrap(),
        read("ocsp_p1_good.der")
    );
    let rep = Handshake::reply(&b, b"perm-b", &pdata(pb), &req.sent, None, None).unwrap();
    assert!(rep.sent.binary("ocsp_status").is_none());
    // participant1 has no status for participant2 from its responder.
    assert!(req.process_reply(&a, &rep.sent).is_err());

    // participant3 is revoked: participant1's responder says so.
    let c = identity("p3", None).unwrap();
    let pc = c.adjusted_prefix(&[3; 16]);
    let req = Handshake::request(&c, b"perm-c", &pdata(pc), None);
    let e = Handshake::reply(&a, b"perm-a", &pdata(pa), &req.sent, None, None).unwrap_err();
    assert!(e.to_string().contains("revoked"), "{e}");
}
