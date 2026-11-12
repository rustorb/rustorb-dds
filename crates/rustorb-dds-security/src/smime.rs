//! Signed documents: S/MIME multipart/signed with a detached CMS
//! SignedData (DDS Security 1.2, 10.4.1.2), as the governance and
//! permissions documents come.

use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerIdentifier};
use der::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::Error;
use crate::pki::Cert;

const OID_SIGNED_DATA: &str = "1.2.840.113549.1.7.2";
const OID_SHA256: &str = "2.16.840.1.101.3.4.2.1";
const OID_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";

fn der_error(e: der::Error) -> Error {
    Error::new(format!("CMS: {e}"))
}

/// Checks the S/MIME document `smime` against `ca` (the signer is the CA
/// or a certificate the CA issued, included in the signature); the signed
/// content, its MIME headers removed.
pub fn verify(smime: &[u8], ca: &Cert) -> Result<Vec<u8>, Error> {
    let text = std::str::from_utf8(smime.strip_suffix(&[0]).unwrap_or(smime))
        .map_err(|_| Error::new("the signed document is not text"))?;
    let (content, signature) = split(text)?;
    let ci = ContentInfo::from_der(&signature).map_err(der_error)?;
    if ci.content_type.to_string() != OID_SIGNED_DATA {
        return Err(Error::new("the signature is not a SignedData"));
    }
    let sd: SignedData = ci.content.decode_as().map_err(der_error)?;
    let certs: Vec<Cert> = sd
        .certificates
        .iter()
        .flat_map(|s| s.0.iter())
        .filter_map(|c| match c {
            cms::cert::CertificateChoices::Certificate(c) => c.to_der().ok(),
            _ => None,
        })
        .filter_map(|d| Cert::parse(&d).ok())
        .collect();
    // The signed octets: the first part, lines ending in CRLF.
    let canonical = crlf(content);
    let mut last = Error::new("the document has no signer");
    for si in sd.signer_infos.0.iter() {
        let signer = match &si.sid {
            SignerIdentifier::IssuerAndSerialNumber(is) => {
                let issuer = is.issuer.to_der().map_err(der_error)?;
                let serial = is.serial_number.as_bytes().to_vec();
                certs
                    .iter()
                    .chain(std::iter::once(ca))
                    .find(|c| c.issuer_and_serial() == (issuer.clone(), serial.clone()))
                    .cloned()
            }
            SignerIdentifier::SubjectKeyIdentifier(_) => None,
        };
        let Some(signer) = signer else {
            last = Error::new("the signer's certificate is not in the signature");
            continue;
        };
        let result = (|| {
            if signer.der() != ca.der() {
                signer.verify_signed_by(ca)?;
            }
            if si.digest_alg.oid.to_string() != OID_SHA256 {
                return Err(Error::new("only SHA-256 digests are supported"));
            }
            let digest = Sha256::digest(canonical.as_bytes());
            let attrs = si
                .signed_attrs
                .as_ref()
                .ok_or_else(|| Error::new("the signer has no signed attributes"))?;
            let md = attrs
                .iter()
                .find(|a| a.oid.to_string() == OID_MESSAGE_DIGEST)
                .and_then(|a| a.values.iter().next())
                .and_then(|v| v.decode_as::<der::asn1::OctetString>().ok())
                .ok_or_else(|| Error::new("no message digest"))?;
            if md.as_bytes() != &digest[..] {
                return Err(Error::new("the document does not match its signature"));
            }
            let signed = attrs.to_der().map_err(der_error)?;
            signer.public_key().verify_with(
                &si.signature_algorithm.oid.to_string(),
                &signed,
                si.signature.as_bytes(),
            )
        })();
        match result {
            Ok(()) => return Ok(strip_headers(content).as_bytes().to_vec()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The first part and the decoded signature of a multipart/signed text.
fn split(text: &str) -> Result<(&str, Vec<u8>), Error> {
    let boundary = text
        .lines()
        .take_while(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split("boundary=")
        .nth(1)
        .map(|b| {
            b.trim_start_matches('"')
                .split(['"', ';', ' '])
                .next()
                .unwrap_or("")
                .to_string()
        })
        .filter(|b| !b.is_empty())
        .ok_or_else(|| Error::new("not a multipart/signed document"))?;
    let delim = format!("--{boundary}");
    let mut parts = text.split(delim.as_str()).skip(1);
    let first = parts.next().ok_or_else(|| Error::new("no signed part"))?;
    let second = parts
        .next()
        .ok_or_else(|| Error::new("no signature part"))?;
    // The part starts after the line break following the delimiter and
    // ends before the line break preceding the next one.
    let first = first
        .strip_prefix("\r\n")
        .or_else(|| first.strip_prefix('\n'))
        .unwrap_or(first);
    let first = first
        .strip_suffix("\r\n")
        .or_else(|| first.strip_suffix('\n'))
        .unwrap_or(first);
    let body = second
        .split_once("\r\n\r\n")
        .or_else(|| second.split_once("\n\n"))
        .map(|(_, b)| b)
        .ok_or_else(|| Error::new("the signature part has no body"))?;
    let b64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let b64 = b64.trim_end_matches('-');
    Ok((first, base64_decode(b64)?))
}

/// `s` with every line ending in CRLF.
fn crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 32);
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(line.strip_suffix('\r').unwrap_or(line));
    }
    out
}

/// The content of a MIME part: after its headers, if it has any.
fn strip_headers(part: &str) -> &str {
    let first = part.lines().next().unwrap_or("");
    let has_headers = first.split_once(':').is_some_and(|(n, _)| {
        !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    if !has_headers {
        return part;
    }
    part.split_once("\r\n\r\n")
        .or_else(|| part.split_once("\n\n"))
        .map(|(_, b)| b)
        .unwrap_or(part)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err(Error::new("the signature is not base64")),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}
