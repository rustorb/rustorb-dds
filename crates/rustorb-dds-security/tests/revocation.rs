//! Encrypted private keys (`dds.sec.auth.password`) and certificate
//! revocation lists (DDS Security 1.2, 10.3.1 and 10.3.3). The files come
//! from certs/gen_crl_and_keys.sh.

use rustorb_dds_security::auth::{Identity, IdentityOptions};
use rustorb_dds_security::pki::{Crl, PrivateKey, uri_password};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");

fn r(f: &str) -> Vec<u8> {
    std::fs::read(format!("{D}{f}")).unwrap()
}

fn s(f: &str) -> String {
    String::from_utf8(r(f)).unwrap()
}

fn identity(
    who: &str,
    key: &str,
    password: Option<&str>,
    crl: Option<&str>,
) -> Result<Identity, String> {
    Identity::with_options(
        &r("identity_ca.pem"),
        &r(&format!("{who}_cert.pem")),
        &r(key),
        &IdentityOptions {
            password: password.map(str::to_string),
            crl: crl.map(r),
            ocsp: None,
        },
    )
    .map_err(|e| e.to_string())
}

#[test]
fn encrypted_private_keys_are_read_with_the_password() {
    let cert = rustorb_dds_security::pki::Cert::parse(&r("p1_cert.pem")).unwrap();
    for f in ["p1_key_pkcs8_enc.pem", "p1_key_dek.pem"] {
        let k = PrivateKey::from_pem_with_password(&s(f), Some("OpenSesame")).unwrap();
        assert!(k.matches(cert.public_key()), "{f}");
        let sig = k.sign(b"data").unwrap();
        cert.public_key().verify(b"data", &sig).unwrap();
    }
    // The key itself in Base64, as Table 50 words the password.
    let raw = base64(&(0..16).collect::<Vec<u8>>());
    let k = PrivateKey::from_pem_with_password(&s("p1_key_raw.pem"), Some(&raw)).unwrap();
    assert!(k.matches(cert.public_key()));
    // RSA in PKCS #1.
    let cert2 = rustorb_dds_security::pki::Cert::parse(&r("p2_cert.pem")).unwrap();
    let k = PrivateKey::from_pem_with_password(&s("p2_key_dek.pem"), Some("OpenSesame")).unwrap();
    assert!(k.matches(cert2.public_key()));
    // An unencrypted key ignores the password.
    assert!(PrivateKey::from_pem_with_password(&s("p1_key.pem"), Some("x")).is_ok());
}

#[test]
fn encrypted_private_keys_need_the_right_password() {
    for f in [
        "p1_key_pkcs8_enc.pem",
        "p1_key_dek.pem",
        "p1_key_raw.pem",
        "p2_key_dek.pem",
    ] {
        let e = PrivateKey::from_pem(&s(f)).unwrap_err().to_string();
        assert!(e.contains("dds.sec.auth.password"), "{f}: {e}");
        let e = PrivateKey::from_pem_with_password(&s(f), Some("wrong"))
            .unwrap_err()
            .to_string();
        assert!(e.contains("does not decrypt"), "{f}: {e}");
    }
}

#[test]
fn the_password_can_be_in_the_uri_of_the_key() {
    assert_eq!(
        uri_password("file:k.pem?password=OpenSesame"),
        Some("OpenSesame")
    );
    assert_eq!(uri_password("file:k.pem?a=b&password=x"), Some("x"));
    assert_eq!(uri_password("file:k.pem"), None);
    assert_eq!(uri_password("data:,?password=x"), None);
    let id = Identity::from_uris_with(
        &format!("file:{D}identity_ca.pem"),
        &format!("file:{D}p1_cert.pem"),
        &format!("file:{D}p1_key_dek.pem?password=OpenSesame"),
        None,
        None,
        None,
    );
    assert!(id.is_ok(), "{:?}", id.err());
}

#[test]
fn identities_with_an_encrypted_key() {
    identity("p1", "p1_key_pkcs8_enc.pem", Some("OpenSesame"), None).unwrap();
    identity("p2", "p2_key_dek.pem", Some("OpenSesame"), None).unwrap();
    assert!(identity("p1", "p1_key_dek.pem", None, None).is_err());
}

#[test]
fn revoked_certificates_are_refused() {
    let crls = Crl::parse_all(&r("crl_p2.pem")).unwrap();
    assert_eq!(crls.len(), 1);
    // Its own certificate revoked: the identity is not valid.
    let e = identity("p2", "p2_key.pem", None, Some("crl_p2.pem")).unwrap_err();
    assert!(e.contains("revoked"), "{e}");
    // A peer's: the handshake refuses it.
    let p1 = identity("p1", "p1_key.pem", None, Some("crl_p2.pem")).unwrap();
    let e = p1.verify_peer(&r("p2_cert.pem")).unwrap_err().to_string();
    assert!(e.contains("revoked"), "{e}");
    // Without the CRL it is accepted.
    let p1 = identity("p1", "p1_key.pem", None, None).unwrap();
    p1.verify_peer(&r("p2_cert.pem")).unwrap();
}

#[test]
fn crls_must_be_current_and_of_the_identity_ca() {
    let e = identity("p1", "p1_key.pem", None, Some("crl_expired.pem")).unwrap_err();
    assert!(e.contains("expired"), "{e}");
    let e = identity("p1", "p1_key.pem", None, Some("crl_other_ca.pem")).unwrap_err();
    assert!(e.contains("not of the identity CA"), "{e}");
    let e = identity("p1", "p1_key.pem", None, Some("p1_cert.pem")).unwrap_err();
    assert!(e.contains("no X509 CRL"), "{e}");
}

fn base64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
