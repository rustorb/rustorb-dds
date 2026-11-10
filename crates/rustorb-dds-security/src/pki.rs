//! X.509 certificates, private keys and digital signatures (DDS Security
//! 1.2, 8.2): RSA 2048 (RSASSA-PSS and PKCS #1 v1.5, SHA-256) and ECDSA
//! P-256 with SHA-256.

use der::{Decode, DecodePem, Encode};
use p256::ecdsa::signature::{Signer, Verifier};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use sha2::{Digest, Sha256};
use spki::SubjectPublicKeyInfoOwned;
use x509_cert::Certificate;

use crate::Error;

const OID_EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
const OID_P256: &str = "1.2.840.10045.3.1.7";
const OID_RSA: &str = "1.2.840.113549.1.1.1";
const OID_ECDSA_SHA256: &str = "1.2.840.10045.4.3.2";
const OID_RSA_SHA256: &str = "1.2.840.113549.1.1.11";
const OID_RSA_PSS: &str = "1.2.840.113549.1.1.10";

/// The digital signature algorithm of a key (Table 25), named as DDS
/// Security 1.1 does, which 1.2 still accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    /// RSASSA-PSS with SHA-256 and MGF1 SHA-256, RSA 2048.
    RsaPss,
    /// ECDSA P-256 with SHA-256.
    EcdsaP256,
}

impl SignatureAlgorithm {
    /// The name sent in `c.dsign_algo` and `dds.cert.algo`.
    pub fn name(self) -> &'static str {
        match self {
            SignatureAlgorithm::RsaPss => "RSASSA-PSS-SHA256",
            SignatureAlgorithm::EcdsaP256 => "ECDSA-SHA256",
        }
    }

    /// From any of its names (1.1 or 1.2).
    pub fn from_name(n: &str) -> Option<Self> {
        match n {
            "RSASSA-PSS-SHA256" | "RSA-2048" | "RSASSA-PSS-MGF1SHA256+2048+SHA256" => {
                Some(SignatureAlgorithm::RsaPss)
            }
            "ECDSA-SHA256" | "EC-prime256v1" | "ECDSA+P256+SHA256" => {
                Some(SignatureAlgorithm::EcdsaP256)
            }
            _ => None,
        }
    }
}

/// A public key of a certificate.
#[derive(Clone, Debug)]
pub enum PublicKey {
    /// RSA.
    Rsa(rsa::RsaPublicKey),
    /// ECDSA P-256.
    P256(p256::ecdsa::VerifyingKey),
}

impl PublicKey {
    fn from_spki(spki: &SubjectPublicKeyInfoOwned) -> Result<Self, Error> {
        let alg = spki.algorithm.oid.to_string();
        let der = spki.to_der().map_err(der_error)?;
        match alg.as_str() {
            OID_RSA => Ok(PublicKey::Rsa(
                rsa::RsaPublicKey::from_public_key_der(&der)
                    .map_err(|e| Error::new(format!("an RSA key: {e}")))?,
            )),
            OID_EC_PUBLIC_KEY => {
                let curve = spki
                    .algorithm
                    .parameters
                    .as_ref()
                    .and_then(|p| p.decode_as::<der::asn1::ObjectIdentifier>().ok())
                    .map(|o| o.to_string());
                if curve.as_deref() != Some(OID_P256) {
                    return Err(Error::new("only the EC curve P-256 is supported"));
                }
                Ok(PublicKey::P256(
                    p256::ecdsa::VerifyingKey::from_sec1_bytes(spki.subject_public_key.raw_bytes())
                        .map_err(|e| Error::new(format!("an EC key: {e}")))?,
                ))
            }
            a => Err(Error::new(format!("unsupported public key algorithm {a}"))),
        }
    }

    /// The signature algorithm of this key.
    pub fn algorithm(&self) -> SignatureAlgorithm {
        match self {
            PublicKey::Rsa(_) => SignatureAlgorithm::RsaPss,
            PublicKey::P256(_) => SignatureAlgorithm::EcdsaP256,
        }
    }

    /// Verifies a signature made by [`PrivateKey::sign`]: RSASSA-PSS (any
    /// salt length tried: the digest's and the largest) or ECDSA (DER,
    /// or the 64 octets r|s).
    pub fn verify(&self, data: &[u8], sig: &[u8]) -> Result<(), Error> {
        let bad = || Error::new("the signature does not verify");
        match self {
            PublicKey::P256(k) => {
                let s = p256::ecdsa::Signature::from_der(sig)
                    .or_else(|_| p256::ecdsa::Signature::from_slice(sig))
                    .map_err(|_| bad())?;
                k.verify(data, &s).map_err(|_| bad())
            }
            PublicKey::Rsa(k) => {
                let hash = Sha256::digest(data);
                let max = rsa::traits::PublicKeyParts::size(k).saturating_sub(32 + 2);
                for salt in [32, max, 0] {
                    if k.verify(rsa::Pss::new_with_salt::<Sha256>(salt), &hash, sig)
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(bad())
            }
        }
    }

    /// Verifies a signature of a certificate or a CMS signer: by `oid`,
    /// ECDSA-SHA256, SHA256-with-RSA or RSASSA-PSS (SHA-256).
    pub fn verify_with(&self, oid: &str, data: &[u8], sig: &[u8]) -> Result<(), Error> {
        let bad = || Error::new("the signature does not verify");
        match (self, oid) {
            (PublicKey::P256(_), OID_ECDSA_SHA256 | OID_EC_PUBLIC_KEY) => self.verify(data, sig),
            (PublicKey::Rsa(k), OID_RSA_SHA256 | OID_RSA) => {
                let hash = Sha256::digest(data);
                k.verify(rsa::Pkcs1v15Sign::new::<Sha256>(), &hash, sig)
                    .map_err(|_| bad())
            }
            (PublicKey::Rsa(_), OID_RSA_PSS) => self.verify(data, sig),
            _ => Err(Error::new(format!("unsupported signature algorithm {oid}"))),
        }
    }
}

/// A private key.
#[derive(Clone)]
pub enum PrivateKey {
    /// RSA.
    Rsa(Box<rsa::RsaPrivateKey>),
    /// ECDSA P-256.
    P256(p256::ecdsa::SigningKey),
}

impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PrivateKey::Rsa(_) => "PrivateKey::Rsa",
            PrivateKey::P256(_) => "PrivateKey::P256",
        })
    }
}

impl PrivateKey {
    /// From PEM: `PRIVATE KEY` (PKCS #8), `EC PRIVATE KEY` (SEC 1) or
    /// `RSA PRIVATE KEY` (PKCS #1), unencrypted.
    pub fn from_pem(pem: &str) -> Result<Self, Error> {
        let (label, der) = pem_rfc7468(pem)?;
        let bad = |e: &dyn std::fmt::Display| Error::new(format!("the private key: {e}"));
        match label.as_str() {
            "PRIVATE KEY" => {
                if let Ok(k) = p256::SecretKey::from_pkcs8_der(&der) {
                    Ok(PrivateKey::P256(k.into()))
                } else {
                    rsa::RsaPrivateKey::from_pkcs8_der(&der)
                        .map(|k| PrivateKey::Rsa(Box::new(k)))
                        .map_err(|e| bad(&e))
                }
            }
            "EC PRIVATE KEY" => p256::SecretKey::from_sec1_der(&der)
                .map(|k| PrivateKey::P256(k.into()))
                .map_err(|e| bad(&e)),
            "RSA PRIVATE KEY" => rsa::RsaPrivateKey::from_pkcs1_der(&der)
                .map(|k| PrivateKey::Rsa(Box::new(k)))
                .map_err(|e| bad(&e)),
            "ENCRYPTED PRIVATE KEY" => Err(Error::new("encrypted private keys are not supported")),
            l => Err(Error::new(format!("not a private key: {l}"))),
        }
    }

    /// The signature algorithm of this key.
    pub fn algorithm(&self) -> SignatureAlgorithm {
        match self {
            PrivateKey::Rsa(_) => SignatureAlgorithm::RsaPss,
            PrivateKey::P256(_) => SignatureAlgorithm::EcdsaP256,
        }
    }

    /// Signs `data`: RSASSA-PSS (SHA-256, salt of 32 octets) or ECDSA
    /// SHA-256 (DER).
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        match self {
            PrivateKey::P256(k) => {
                let s: p256::ecdsa::Signature = k.sign(data);
                Ok(s.to_der().as_bytes().to_vec())
            }
            PrivateKey::Rsa(k) => {
                let hash = Sha256::digest(data);
                k.sign_with_rng(
                    &mut rand_core::OsRng,
                    rsa::Pss::new_with_salt::<Sha256>(32),
                    &hash,
                )
                .map_err(|e| Error::new(format!("signing: {e}")))
            }
        }
    }

    /// Whether `public` is this key's.
    pub fn matches(&self, public: &PublicKey) -> bool {
        match (self, public) {
            (PrivateKey::P256(k), PublicKey::P256(p)) => k.verifying_key() == p,
            (PrivateKey::Rsa(k), PublicKey::Rsa(p)) => &k.to_public_key() == p,
            _ => false,
        }
    }
}

/// The label and DER of the first PEM block in `pem`.
pub fn pem_rfc7468(pem: &str) -> Result<(String, Vec<u8>), Error> {
    let start = pem
        .find("-----BEGIN ")
        .ok_or_else(|| Error::new("no PEM block"))?;
    let (label, der) = pem_rfc7468::decode_vec(&pem.as_bytes()[start..])
        .map_err(|e| Error::new(format!("PEM: {e}")))?;
    Ok((label.to_string(), der))
}

fn der_error(e: der::Error) -> Error {
    Error::new(format!("DER: {e}"))
}

/// An X.509 certificate.
#[derive(Clone, Debug)]
pub struct Cert {
    cert: Certificate,
    der: Vec<u8>,
    public: PublicKey,
}

impl Cert {
    /// From PEM, DER, or PEM with a NUL at the end.
    pub fn parse(b: &[u8]) -> Result<Self, Error> {
        let b = b.strip_suffix(&[0]).unwrap_or(b);
        let cert = match std::str::from_utf8(b) {
            Ok(s) if s.contains("-----BEGIN") => {
                let start = s.find("-----BEGIN").expect("found");
                Certificate::from_pem(&s[start..]).map_err(der_error)?
            }
            _ => Certificate::from_der(b).map_err(der_error)?,
        };
        let der = cert.to_der().map_err(der_error)?;
        let public = PublicKey::from_spki(&cert.tbs_certificate.subject_public_key_info)?;
        Ok(Cert { cert, der, public })
    }

    /// Its DER.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// Its PEM.
    pub fn pem(&self) -> String {
        pem_rfc7468::encode_string("CERTIFICATE", pem_rfc7468::LineEnding::LF, &self.der)
            .expect("a certificate encodes")
    }

    /// Its public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// The DER of its subject name.
    pub fn subject_der(&self) -> Vec<u8> {
        self.cert
            .tbs_certificate
            .subject
            .to_der()
            .expect("a name encodes")
    }

    /// Its subject name, as text (RFC 4514).
    pub fn subject(&self) -> String {
        self.cert.tbs_certificate.subject.to_string()
    }

    /// The DER of its issuer name.
    fn issuer_der(&self) -> Vec<u8> {
        self.cert
            .tbs_certificate
            .issuer
            .to_der()
            .expect("a name encodes")
    }

    /// Whether it is valid at `now` (seconds since the Unix epoch).
    pub fn is_valid_at(&self, now: u64) -> bool {
        let v = &self.cert.tbs_certificate.validity;
        v.not_before.to_unix_duration().as_secs() <= now
            && now <= v.not_after.to_unix_duration().as_secs()
    }

    /// Checks that `issuer` signed it and that it is valid now.
    pub fn verify_signed_by(&self, issuer: &Cert) -> Result<(), Error> {
        if self.issuer_der() != issuer.subject_der() {
            return Err(Error::new(format!(
                "{} is not issued by {}",
                self.subject(),
                issuer.subject()
            )));
        }
        let tbs = self.cert.tbs_certificate.to_der().map_err(der_error)?;
        let sig = self
            .cert
            .signature
            .as_bytes()
            .ok_or_else(|| Error::new("the signature is not whole octets"))?;
        issuer
            .public
            .verify_with(&self.cert.signature_algorithm.oid.to_string(), &tbs, sig)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if !self.is_valid_at(now) {
            return Err(Error::new(format!("{} is not valid now", self.subject())));
        }
        Ok(())
    }
}

/// Reads a URI of a certificate, key or document (10.3.1): `file:` or
/// `data:,`.
pub fn read_uri(uri: &str) -> Result<Vec<u8>, Error> {
    if let Some(d) = uri.strip_prefix("data:,") {
        Ok(d.as_bytes().to_vec())
    } else if let Some(p) = uri.strip_prefix("file:") {
        let p = p.split('?').next().unwrap_or(p);
        let p = p.strip_prefix("//").unwrap_or(p);
        std::fs::read(p).map_err(|e| Error::new(format!("{p}: {e}")))
    } else {
        Err(Error::new(format!("unsupported URI {uri}")))
    }
}
