//! X.509 certificates, private keys and digital signatures (DDS Security
//! 1.2, 8.2): RSA 2048 (RSASSA-PSS and PKCS #1 v1.5, SHA-256) and ECDSA
//! P-256 with SHA-256.

use base64ct::Encoding as _;
use der::{Decode, DecodePem, Encode};
use p256::ecdsa::signature::{Signer, Verifier};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use sha2::{Digest, Sha256};
use spki::SubjectPublicKeyInfoOwned;
use x509_cert::Certificate;
use x509_cert::crl::CertificateList;

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
        Self::from_pem_with_password(pem, None)
    }

    /// From PEM, encrypted or not. `password` decrypts an encrypted key
    /// (the property `dds.sec.auth.password`, 10.3.1):
    ///
    /// - `ENCRYPTED PRIVATE KEY` (PKCS #8, PBES2 of RFC 8018), with the
    ///   password as its passphrase;
    /// - an `EC PRIVATE KEY`, `RSA PRIVATE KEY` or `PRIVATE KEY` with the
    ///   headers `Proc-Type: 4,ENCRYPTED` and `DEK-Info` (RFC 1421) and
    ///   AES-128, -192 or -256 in CBC mode: the key is derived from the
    ///   password as a passphrase, with MD5 and the first 8 octets of the
    ///   IV (as OpenSSL writes these, and as Cyclone DDS and Fast DDS read
    ///   them); failing that, with AES-128-CBC, the password is taken as
    ///   the Base64 of the key itself, as Table 50 words it.
    ///
    /// The password is ignored for an unencrypted key.
    pub fn from_pem_with_password(pem: &str, password: Option<&str>) -> Result<Self, Error> {
        let block = PemBlock::parse(pem)?;
        let bad = |e: &dyn std::fmt::Display| Error::new(format!("the private key: {e}"));
        let need = || Error::new("the private key is encrypted: dds.sec.auth.password is needed");
        if block.label == "ENCRYPTED PRIVATE KEY" {
            let password = password.ok_or_else(need)?;
            let info = pkcs8::EncryptedPrivateKeyInfo::try_from(block.der.as_slice())
                .map_err(|e| bad(&e))?;
            let doc = info
                .decrypt(password)
                .map_err(|_| Error::new("the private key: the password does not decrypt it"))?;
            return Self::from_der("PRIVATE KEY", doc.as_bytes());
        }
        match block.dek_info()? {
            None => Self::from_der(&block.label, &block.der),
            Some((cipher, iv)) => {
                let password = password.ok_or_else(need)?;
                let mut keys = vec![bytes_to_key(password.as_bytes(), &iv, cipher.key_len())];
                if cipher == Cbc::Aes128
                    && let Ok(k) = base64ct::Base64::decode_vec(password.trim())
                    && k.len() == 16
                {
                    keys.push(k);
                }
                for key in keys {
                    if let Some(der) = cipher.decrypt(&key, &iv, &block.der)
                        && let Ok(k) = Self::from_der(&block.label, &der)
                    {
                        return Ok(k);
                    }
                }
                Err(Error::new(
                    "the private key: the password does not decrypt it",
                ))
            }
        }
    }

    /// From the DER of a PEM block labelled `label`.
    fn from_der(label: &str, der: &[u8]) -> Result<Self, Error> {
        let bad = |e: &dyn std::fmt::Display| Error::new(format!("the private key: {e}"));
        match label {
            "PRIVATE KEY" => {
                if let Ok(k) = p256::SecretKey::from_pkcs8_der(der) {
                    Ok(PrivateKey::P256(k.into()))
                } else {
                    rsa::RsaPrivateKey::from_pkcs8_der(der)
                        .map(|k| PrivateKey::Rsa(Box::new(k)))
                        .map_err(|e| bad(&e))
                }
            }
            "EC PRIVATE KEY" => p256::SecretKey::from_sec1_der(der)
                .map(|k| PrivateKey::P256(k.into()))
                .map_err(|e| bad(&e)),
            "RSA PRIVATE KEY" => rsa::RsaPrivateKey::from_pkcs1_der(der)
                .map(|k| PrivateKey::Rsa(Box::new(k)))
                .map_err(|e| bad(&e)),
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

/// A PEM block that may have the headers of RFC 1421 (which RFC 7468
/// leaves out).
struct PemBlock {
    label: String,
    headers: Vec<(String, String)>,
    der: Vec<u8>,
}

impl PemBlock {
    /// The first block in `pem`.
    fn parse(pem: &str) -> Result<Self, Error> {
        let start = pem
            .find("-----BEGIN ")
            .ok_or_else(|| Error::new("no PEM block"))?;
        let rest = &pem[start + "-----BEGIN ".len()..];
        let (label, rest) = rest
            .split_once("-----")
            .ok_or_else(|| Error::new("PEM: a bad BEGIN line"))?;
        let end = format!("-----END {label}-----");
        let body = &rest[..rest
            .find(&end)
            .ok_or_else(|| Error::new(format!("PEM: no {end}")))?];
        let mut headers = Vec::new();
        let mut text = String::new();
        let mut in_headers = true;
        for line in body.lines().map(str::trim) {
            if line.is_empty() {
                // The blank line after the headers.
                in_headers &= headers.is_empty() && text.is_empty();
                continue;
            }
            match line.split_once(':') {
                Some((k, v)) if in_headers => headers.push((k.trim().into(), v.trim().into())),
                _ => {
                    in_headers = false;
                    text.push_str(line);
                }
            }
        }
        let der =
            base64ct::Base64::decode_vec(&text).map_err(|e| Error::new(format!("PEM: {e}")))?;
        Ok(PemBlock {
            label: label.to_string(),
            headers,
            der,
        })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The cipher and IV of an encrypted block (RFC 1421, 4.6.1.1 and
    /// 4.6.1.3), `None` if it is not encrypted.
    fn dek_info(&self) -> Result<Option<(Cbc, Vec<u8>)>, Error> {
        let encrypted = self
            .header("Proc-Type")
            .is_some_and(|v| v.split(',').any(|p| p.trim() == "ENCRYPTED"));
        if !encrypted {
            return Ok(None);
        }
        let dek = self
            .header("DEK-Info")
            .ok_or_else(|| Error::new("the private key: an encrypted PEM with no DEK-Info"))?;
        let (alg, iv) = dek
            .split_once(',')
            .ok_or_else(|| Error::new(format!("the private key: DEK-Info {dek}")))?;
        let cipher = match alg.trim() {
            "AES-128-CBC" => Cbc::Aes128,
            "AES-192-CBC" => Cbc::Aes192,
            "AES-256-CBC" => Cbc::Aes256,
            a => {
                return Err(Error::new(format!(
                    "the private key: unsupported cipher {a} (AES-128/192/256-CBC are)"
                )));
            }
        };
        let iv = hex(iv.trim())
            .filter(|iv| iv.len() == 16)
            .ok_or_else(|| Error::new(format!("the private key: DEK-Info {dek}")))?;
        Ok(Some((cipher, iv)))
    }
}

fn hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The ciphers of an encrypted PEM block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cbc {
    Aes128,
    Aes192,
    Aes256,
}

impl Cbc {
    fn key_len(self) -> usize {
        match self {
            Cbc::Aes128 => 16,
            Cbc::Aes192 => 24,
            Cbc::Aes256 => 32,
        }
    }

    /// Decrypts with PKCS #7 padding; `None` if the padding is wrong.
    fn decrypt(self, key: &[u8], iv: &[u8], data: &[u8]) -> Option<Vec<u8>> {
        use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
        fn run<C: BlockDecryptMut + KeyIvInit>(
            key: &[u8],
            iv: &[u8],
            data: &[u8],
        ) -> Option<Vec<u8>> {
            C::new_from_slices(key, iv)
                .ok()?
                .decrypt_padded_vec_mut::<Pkcs7>(data)
                .ok()
        }
        match self {
            Cbc::Aes128 => run::<cbc::Decryptor<aes::Aes128>>(key, iv, data),
            Cbc::Aes192 => run::<cbc::Decryptor<aes::Aes192>>(key, iv, data),
            Cbc::Aes256 => run::<cbc::Decryptor<aes::Aes256>>(key, iv, data),
        }
    }
}

/// The key of an encrypted PEM block from a passphrase: MD5 over the
/// passphrase and the salt (the first 8 octets of the IV), then over the
/// previous digest, the passphrase and the salt, until there are `len`
/// octets (PBKDF1 of RFC 8018 with one iteration, extended past one
/// digest).
fn bytes_to_key(password: &[u8], iv: &[u8], len: usize) -> Vec<u8> {
    use md5::{Digest as _, Md5};
    let salt = &iv[..8];
    let mut key = Vec::with_capacity(len + 16);
    let mut prev: Vec<u8> = Vec::new();
    while key.len() < len {
        let mut h = Md5::new();
        h.update(&prev);
        h.update(password);
        h.update(salt);
        prev = h.finalize().to_vec();
        key.extend_from_slice(&prev);
    }
    key.truncate(len);
    key
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

    /// Its subject name in the form `/C=JP/O=Org/CN=Name`, its relative
    /// names in certificate order: as the subject names of the tokens
    /// are written by Fast DDS, which compares them octet by octet.
    pub fn subject_slashed(&self) -> String {
        self.cert
            .tbs_certificate
            .subject
            .0
            .iter()
            .map(|rdn| {
                let parts: Vec<String> = rdn.0.iter().map(|a| a.to_string()).collect();
                format!("/{}", parts.join("+"))
            })
            .collect()
    }

    /// The DER of its issuer name and its serial number, which name it
    /// in a CMS signer.
    pub fn issuer_and_serial(&self) -> (Vec<u8>, Vec<u8>) {
        (
            self.issuer_der(),
            self.cert.tbs_certificate.serial_number.as_bytes().to_vec(),
        )
    }

    /// The parsed certificate.
    pub(crate) fn inner(&self) -> &Certificate {
        &self.cert
    }

    /// The DER of its issuer name.
    pub(crate) fn issuer_der(&self) -> Vec<u8> {
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
        if !self.is_valid_at(unix_now()) {
            return Err(Error::new(format!("{} is not valid now", self.subject())));
        }
        Ok(())
    }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A certificate revocation list (RFC 5280, 5), which 10.3.3 has the
/// plugin check, with validate_local_identity and validate_remote_identity.
#[derive(Clone, Debug)]
pub struct Crl {
    list: CertificateList,
}

impl Crl {
    /// The CRLs in `b`: PEM blocks `X509 CRL`, or one in DER.
    pub fn parse_all(b: &[u8]) -> Result<Vec<Self>, Error> {
        let b = b.strip_suffix(&[0]).unwrap_or(b);
        let lists = match std::str::from_utf8(b) {
            Ok(s) if s.contains("-----BEGIN") => {
                let mut lists = Vec::new();
                let mut rest = s;
                while let Some(i) = rest.find("-----BEGIN ") {
                    let block = PemBlock::parse(&rest[i..])?;
                    let end = format!("-----END {}-----", block.label);
                    rest = &rest[i + rest[i..].find(&end).expect("parsed") + end.len()..];
                    if block.label == "X509 CRL" {
                        lists.push(CertificateList::from_der(&block.der).map_err(der_error)?);
                    }
                }
                lists
            }
            _ => vec![CertificateList::from_der(b).map_err(der_error)?],
        };
        if lists.is_empty() {
            return Err(Error::new("no X509 CRL in the CRL file"));
        }
        Ok(lists.into_iter().map(|list| Crl { list }).collect())
    }

    /// Its issuer, as text.
    pub fn issuer(&self) -> String {
        self.list.tbs_cert_list.issuer.to_string()
    }

    /// Whether `issuer` signed it, and whether it is current at `now`
    /// (seconds since the Unix epoch): issued, and its next update, if
    /// it names one, not passed.
    pub fn verify_signed_by(&self, issuer: &Cert, now: u64) -> Result<(), Error> {
        let tbs = &self.list.tbs_cert_list;
        if tbs.issuer.to_der().map_err(der_error)? != issuer.subject_der() {
            return Err(Error::new(format!(
                "the CRL of {} is not of the identity CA {}",
                self.issuer(),
                issuer.subject()
            )));
        }
        let sig = self
            .list
            .signature
            .as_bytes()
            .ok_or_else(|| Error::new("the CRL's signature is not whole octets"))?;
        issuer
            .public
            .verify_with(
                &self.list.signature_algorithm.oid.to_string(),
                &tbs.to_der().map_err(der_error)?,
                sig,
            )
            .map_err(|_| Error::new(format!("the CRL of {} does not verify", self.issuer())))?;
        if tbs.this_update.to_unix_duration().as_secs() > now {
            return Err(Error::new(format!(
                "the CRL of {} is not valid yet",
                self.issuer()
            )));
        }
        if let Some(next) = tbs.next_update
            && next.to_unix_duration().as_secs() < now
        {
            return Err(Error::new(format!(
                "the CRL of {} has expired (its next update was {next})",
                self.issuer()
            )));
        }
        Ok(())
    }

    /// Whether it revokes `cert`: same issuer, and its serial number listed.
    pub fn revokes(&self, cert: &Cert) -> bool {
        let tbs = &self.list.tbs_cert_list;
        let serial = cert.cert.tbs_certificate.serial_number.as_bytes();
        tbs.issuer.to_der().ok() == Some(cert.issuer_der())
            && tbs
                .revoked_certificates
                .iter()
                .flatten()
                .any(|r| r.serial_number.as_bytes() == serial)
    }
}

/// The password in the query of a `file:` URI of a private key, as in
/// the example `file:identity_ca_private_key.pem?password=OpenSesame` of
/// Table 50.
pub fn uri_password(uri: &str) -> Option<&str> {
    let (_, query) = uri.strip_prefix("file:")?.split_once('?')?;
    query.split('&').find_map(|p| p.strip_prefix("password="))
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
