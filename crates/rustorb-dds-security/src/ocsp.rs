//! OCSP (RFC 6960), which DDS Security 1.2 has the builtin authentication
//! plugin use besides CRLs (10.3.3, validate_local_identity): the status of
//! an identity certificate, asked of a responder over HTTP or stapled to a
//! handshake message (the `ocsp_status` property, 10.3.2.5).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use der::asn1::{Null, ObjectIdentifier, OctetString};
use der::{Decode, Encode};
use sha2::Digest;
use spki::AlgorithmIdentifierOwned;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{AuthorityInfoAccessSyntax, ExtendedKeyUsage};
use x509_ocsp::{BasicOcspResponse, CertId, OcspRequest, OcspResponseStatus, Request, TbsRequest};

use crate::Error;
use crate::pki::Cert;

const OID_SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");
const OID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const OID_OCSP_BASIC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1.1");
const OID_AD_OCSP: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1");
const OID_KP_OCSP_SIGNING: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.9");
const OID_AIA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.1.1");
const OID_EKU: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.37");

/// How far the clocks of the responder and of this host may differ, in
/// seconds.
const SKEW: u64 = 300;

/// How a participant uses OCSP, besides the responses stapled to the
/// handshake messages of its peers, which it always takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcspOptions {
    /// The responder's URL (`http://host[:port]/path`); `None` takes the
    /// one in each certificate's Authority Information Access.
    pub responder: Option<String>,
    /// How long a query may take.
    pub timeout: Duration,
    /// How often the participant asks again for its own status (stapled
    /// to its handshakes) and for those of its authenticated peers: a
    /// revoked peer is revoked as `on_revoke_identity` does. Zero asks
    /// only at authentication.
    pub refresh: Duration,
}

impl Default for OcspOptions {
    fn default() -> Self {
        OcspOptions {
            responder: None,
            timeout: Duration::from_secs(3),
            refresh: Duration::from_secs(300),
        }
    }
}

/// The status of a certificate (RFC 6960, 4.2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertStatus {
    /// Not revoked.
    Good,
    /// Revoked.
    Revoked,
    /// Unknown to the responder.
    Unknown,
}

/// The status a response gives a certificate, and until when it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    /// The status.
    pub status: CertStatus,
    /// The nextUpdate of the response (seconds since the Unix epoch), if
    /// it has one.
    pub next_update: Option<u64>,
}

/// An OCSP response with a BasicOCSPResponse.
#[derive(Clone, Debug)]
pub struct OcspResponse {
    der: Vec<u8>,
    basic: BasicOcspResponse,
}

fn der_error(e: der::Error) -> Error {
    Error::new(format!("OCSP: {e}"))
}

fn digest(alg: &ObjectIdentifier, data: &[u8]) -> Result<Vec<u8>, Error> {
    if *alg == OID_SHA1 {
        Ok(sha1::Sha1::digest(data).to_vec())
    } else if *alg == OID_SHA256 {
        Ok(sha2::Sha256::digest(data).to_vec())
    } else {
        Err(Error::new(format!(
            "OCSP: unsupported hash algorithm {alg}"
        )))
    }
}

/// The CertID of `cert`, issued by `issuer`, with the hash `alg`.
fn cert_id(cert: &Cert, issuer: &Cert, alg: ObjectIdentifier) -> Result<CertId, Error> {
    let key = issuer
        .inner()
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();
    Ok(CertId {
        hash_algorithm: AlgorithmIdentifierOwned {
            oid: alg,
            parameters: Some(Null.into()),
        },
        issuer_name_hash: OctetString::new(digest(&alg, &cert.issuer_der())?).map_err(der_error)?,
        issuer_key_hash: OctetString::new(digest(&alg, key)?).map_err(der_error)?,
        serial_number: cert.inner().tbs_certificate.serial_number.clone(),
    })
}

fn same_cert_id(a: &CertId, b: &CertId) -> bool {
    a.hash_algorithm.oid == b.hash_algorithm.oid
        && a.issuer_name_hash == b.issuer_name_hash
        && a.issuer_key_hash == b.issuer_key_hash
        && a.serial_number == b.serial_number
}

/// The DER of an OCSPRequest for `cert`, issued by `issuer`: one request,
/// SHA-1 CertID, no nonce, unsigned.
pub fn request_der(cert: &Cert, issuer: &Cert) -> Result<Vec<u8>, Error> {
    OcspRequest {
        tbs_request: TbsRequest {
            request_list: vec![Request {
                req_cert: cert_id(cert, issuer, OID_SHA1)?,
                single_request_extensions: None,
            }],
            ..TbsRequest::default()
        },
        optional_signature: None,
    }
    .to_der()
    .map_err(der_error)
}

/// The OCSP responder named by the Authority Information Access of
/// `cert`, if it has an `http:` one.
pub fn responder_of(cert: &Cert) -> Option<String> {
    let ext = cert
        .inner()
        .tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        .find(|e| e.extn_id == OID_AIA)?;
    let aia = AuthorityInfoAccessSyntax::from_der(ext.extn_value.as_bytes()).ok()?;
    aia.0.iter().find_map(|d| match &d.access_location {
        GeneralName::UniformResourceIdentifier(u)
            if d.access_method == OID_AD_OCSP && u.as_str().starts_with("http://") =>
        {
            Some(u.as_str().to_string())
        }
        _ => None,
    })
}

impl OcspResponse {
    /// From the DER of an OCSPResponse: it must be successful and carry a
    /// BasicOCSPResponse.
    pub fn parse(der: &[u8]) -> Result<Self, Error> {
        let r = x509_ocsp::OcspResponse::from_der(der).map_err(der_error)?;
        if r.response_status != OcspResponseStatus::Successful {
            return Err(Error::new(format!(
                "OCSP: the responder answered {:?}",
                r.response_status
            )));
        }
        let bytes = r
            .response_bytes
            .ok_or_else(|| Error::new("OCSP: a successful response with no bytes"))?;
        if bytes.response_type != OID_OCSP_BASIC {
            return Err(Error::new(format!(
                "OCSP: unsupported response type {}",
                bytes.response_type
            )));
        }
        let basic = BasicOcspResponse::from_der(bytes.response.as_bytes()).map_err(der_error)?;
        Ok(OcspResponse {
            der: der.to_vec(),
            basic,
        })
    }

    /// Its DER.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// Checks that `issuer`, or a responder `issuer` delegated OCSP
    /// signing to (with the extended key usage id-kp-OCSPSigning, its
    /// certificate in the response), signed it; then the status it gives
    /// `cert`, which `issuer` issued, at `now` (seconds since the Unix
    /// epoch). A response not current at `now` is an error.
    pub fn check(&self, cert: &Cert, issuer: &Cert, now: u64) -> Result<Checked, Error> {
        self.verify_signature(issuer)?;
        let single = self
            .basic
            .tbs_response_data
            .responses
            .iter()
            .find(|s| {
                cert_id(cert, issuer, s.cert_id.hash_algorithm.oid)
                    .is_ok_and(|id| same_cert_id(&id, &s.cert_id))
            })
            .ok_or_else(|| {
                Error::new(format!(
                    "OCSP: the response is not about the certificate of {}",
                    cert.subject()
                ))
            })?;
        let this = single.this_update.0.to_unix_duration().as_secs();
        if this > now + SKEW {
            return Err(Error::new("OCSP: the response is not valid yet"));
        }
        let next = single.next_update.map(|t| t.0.to_unix_duration().as_secs());
        if next.is_some_and(|n| n + SKEW < now) {
            return Err(Error::new("OCSP: the response has expired"));
        }
        Ok(Checked {
            status: match single.cert_status {
                x509_ocsp::CertStatus::Good(_) => CertStatus::Good,
                x509_ocsp::CertStatus::Revoked(_) => CertStatus::Revoked,
                x509_ocsp::CertStatus::Unknown(_) => CertStatus::Unknown,
            },
            next_update: next,
        })
    }

    fn verify_signature(&self, issuer: &Cert) -> Result<(), Error> {
        let b = &self.basic;
        let tbs = b.tbs_response_data.to_der().map_err(der_error)?;
        let sig = b
            .signature
            .as_bytes()
            .ok_or_else(|| Error::new("OCSP: the signature is not whole octets"))?;
        let oid = b.signature_algorithm.oid.to_string();
        if issuer.public_key().verify_with(&oid, &tbs, sig).is_ok() {
            return Ok(());
        }
        for c in b.certs.iter().flatten() {
            let Ok(c) = c.to_der().map_err(der_error).and_then(|d| Cert::parse(&d)) else {
                continue;
            };
            if c.verify_signed_by(issuer).is_ok()
                && signs_ocsp(&c)
                && c.public_key().verify_with(&oid, &tbs, sig).is_ok()
            {
                return Ok(());
            }
        }
        Err(Error::new(format!(
            "OCSP: the response is not signed by {} or a responder it delegated to",
            issuer.subject()
        )))
    }
}

/// Whether `c` has the extended key usage id-kp-OCSPSigning.
fn signs_ocsp(c: &Cert) -> bool {
    c.inner()
        .tbs_certificate
        .extensions
        .iter()
        .flatten()
        .filter(|e| e.extn_id == OID_EKU)
        .filter_map(|e| ExtendedKeyUsage::from_der(e.extn_value.as_bytes()).ok())
        .any(|eku| eku.0.contains(&OID_KP_OCSP_SIGNING))
}

/// Asks the responder at `url` (`http://host[:port]/path`) for the status
/// of `cert`, issued by `issuer`, with an HTTP POST (RFC 6960, A.1). The
/// response is parsed, not checked: see [`OcspResponse::check`].
pub fn query(
    url: &str,
    cert: &Cert,
    issuer: &Cert,
    timeout: Duration,
) -> Result<OcspResponse, Error> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| Error::new(format!("OCSP: not an http: URL: {url}")))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
        && !authority.ends_with(']')
    {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let io = |e: std::io::Error| Error::new(format!("OCSP: {url}: {e}"));
    let to = addr
        .to_socket_addrs()
        .map_err(io)?
        .next()
        .ok_or_else(|| Error::new(format!("OCSP: {url}: no address")))?;
    let mut s = TcpStream::connect_timeout(&to, timeout).map_err(io)?;
    s.set_read_timeout(Some(timeout)).map_err(io)?;
    s.set_write_timeout(Some(timeout)).map_err(io)?;
    let body = request_der(cert, issuer)?;
    let head = format!(
        "POST {path} HTTP/1.0\r\nHost: {authority}\r\nContent-Type: application/ocsp-request\r\nAccept: application/ocsp-response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).map_err(io)?;
    s.write_all(&body).map_err(io)?;
    let mut answer = Vec::new();
    s.read_to_end(&mut answer).map_err(io)?;
    let end = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| Error::new(format!("OCSP: {url}: not an HTTP response")))?;
    let head = String::from_utf8_lossy(&answer[..end]);
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(Error::new(format!(
            "OCSP: {url}: {}",
            head.lines().next().unwrap_or("")
        )));
    }
    let mut body = &answer[end + 4..];
    if let Some(n) = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| v.trim().parse::<usize>().ok())?
    }) {
        body = body.get(..n).unwrap_or(body);
    }
    OcspResponse::parse(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_the_certificate_with_sha1() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");
        let r = |f: &str| std::fs::read(format!("{dir}{f}")).unwrap();
        let ca = Cert::parse(&r("identity_ca.pem")).unwrap();
        let p1 = Cert::parse(&r("p1_cert.pem")).unwrap();
        let der = request_der(&p1, &ca).unwrap();
        let req = OcspRequest::from_der(&der).unwrap();
        let id = &req.tbs_request.request_list[0].req_cert;
        assert_eq!(id.hash_algorithm.oid, OID_SHA1);
        assert_eq!(id.issuer_name_hash.as_bytes().len(), 20);
        assert!(same_cert_id(id, &cert_id(&p1, &ca, OID_SHA1).unwrap()));
        assert!(responder_of(&p1).is_none());
    }
}
