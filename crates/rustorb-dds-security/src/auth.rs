//! The builtin authentication plugin, DDS:Auth:PKI-DH (DDS Security 1.2,
//! 10.3): the local identity, the IdentityToken, the adjusted GUID, and
//! the three-message handshake that authenticates two participants and
//! gives them a shared secret.

use rand_core::{OsRng, RngCore};
use rsa::BigUint;
use sha2::{Digest, Sha256};

use crate::Error;
use crate::pki::{Cert, PrivateKey, read_uri};
use crate::token::{BinaryProperty, DataHolder, binary_properties_cdr};

/// The class of the plugin's tokens, without a version.
pub const PLUGIN_CLASS: &str = "DDS:Auth:PKI-DH";
/// The class_id of the IdentityToken this side sends. DDS Security 1.1
/// peers name it so; 1.2 peers compare only the class and the major
/// version.
pub const IDENTITY_TOKEN_CLASS: &str = "DDS:Auth:PKI-DH:1.0";
/// The class_id of the AuthRequestMessageToken.
pub const AUTH_REQUEST_CLASS: &str = "DDS:Auth:PKI-DH:1.0+AuthReq";
/// The class_id of the HandshakeRequestMessageToken.
pub const REQUEST_CLASS: &str = "DDS:Auth:PKI-DH:1.0+Req";
/// The class_id of the HandshakeReplyMessageToken.
pub const REPLY_CLASS: &str = "DDS:Auth:PKI-DH:1.0+Reply";
/// The class_id of the HandshakeFinalMessageToken.
pub const FINAL_CLASS: &str = "DDS:Auth:PKI-DH:1.0+Final";

/// Whether a class_id is of this plugin, major version 1.
pub fn is_pki_dh(class_id: &str) -> bool {
    class_id
        .rsplit_once(':')
        .is_some_and(|(c, v)| c == PLUGIN_CLASS && v.split('.').next() == Some("1"))
}

/// The local identity: the identity CA, this participant's certificate
/// and private key (10.3.1).
#[derive(Clone, Debug)]
pub struct Identity {
    ca: Cert,
    cert: Cert,
    key: PrivateKey,
    /// The key agreement this side asks for when it starts a handshake.
    pub key_agreement: KeyAgreement,
}

impl Identity {
    /// From the PEM of the identity CA, the identity certificate and the
    /// private key; checks the certificate against the CA.
    pub fn new(ca_pem: &[u8], cert_pem: &[u8], key_pem: &[u8]) -> Result<Self, Error> {
        let ca = Cert::parse(ca_pem)?;
        let cert = Cert::parse(cert_pem)?;
        cert.verify_signed_by(&ca)?;
        let key = PrivateKey::from_pem(
            std::str::from_utf8(key_pem).map_err(|_| Error::new("the private key is not PEM"))?,
        )?;
        if !key.matches(cert.public_key()) {
            return Err(Error::new("the private key is not the certificate's"));
        }
        Ok(Identity {
            ca,
            cert,
            key,
            key_agreement: KeyAgreement::EcdhP256,
        })
    }

    /// From the URIs of the properties `dds.sec.auth.identity_ca`,
    /// `dds.sec.auth.identity_certificate` and `dds.sec.auth.private_key`.
    pub fn from_uris(ca: &str, cert: &str, key: &str) -> Result<Self, Error> {
        Self::new(&read_uri(ca)?, &read_uri(cert)?, &read_uri(key)?)
    }

    /// The identity certificate.
    pub fn certificate(&self) -> &Cert {
        &self.cert
    }

    /// The identity CA.
    pub fn ca(&self) -> &Cert {
        &self.ca
    }

    /// The private key.
    pub fn private_key(&self) -> &PrivateKey {
        &self.key
    }

    /// The GUID prefix this participant must use (10.3.3,
    /// validate_local_identity): bit 0 set, 47 bits of the hash of the
    /// subject name, 48 bits of the hash of the candidate GUID.
    pub fn adjusted_prefix(&self, candidate_guid: &[u8; 16]) -> [u8; 12] {
        let s = Sha256::digest(self.cert.subject_der());
        let c = Sha256::digest(candidate_guid);
        let mut p = [0; 12];
        p[0] = (s[0] >> 1) | 0x80;
        // Bits 1..47 are the first 47 bits of the hash: shift right by one.
        for i in 1..6 {
            p[i] = (s[i - 1] << 7) | (s[i] >> 1);
        }
        p[6..].copy_from_slice(&c[..6]);
        p
    }

    /// The IdentityToken (10.3.2.1).
    pub fn identity_token(&self) -> DataHolder {
        let algo = self.key.algorithm().name();
        let ca_algo = self.ca.public_key().algorithm().name();
        DataHolder::new(IDENTITY_TOKEN_CLASS)
            .with_property("dds.cert.sn", &self.cert.subject_slashed())
            .with_property("dds.cert.algo", algo)
            .with_property("dds.ca.sn", &self.ca.subject_slashed())
            .with_property("dds.ca.algo", ca_algo)
    }

    /// Checks a peer's certificate against the identity CA.
    pub fn verify_peer(&self, cert: &[u8]) -> Result<Cert, Error> {
        let c = Cert::parse(cert)?;
        c.verify_signed_by(&self.ca)?;
        Ok(c)
    }
}

/// Checks that `prefix` is what a holder of `cert` may use: bit 0 set and
/// bits 1..47 from the hash of the subject name (10.3.3,
/// begin_handshake_reply).
pub fn check_prefix(prefix: &[u8; 12], cert: &Cert) -> Result<(), Error> {
    let s = Sha256::digest(cert.subject_der());
    let mut want = [0u8; 6];
    want[0] = (s[0] >> 1) | 0x80;
    for i in 1..6 {
        want[i] = (s[i - 1] << 7) | (s[i] >> 1);
    }
    if prefix[..6] == want {
        Ok(())
    } else {
        Err(Error::new(
            "the GUID does not belong to the certificate's subject",
        ))
    }
}

/// A key agreement algorithm (Table 26).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAgreement {
    /// DHE+MODP-2048-256.
    Dh2048,
    /// ECDHE-CEUM+P256.
    EcdhP256,
}

impl KeyAgreement {
    /// The name sent in `c.kagree_algo` (the DDS Security 1.1 name).
    pub fn name(self) -> &'static str {
        match self {
            KeyAgreement::Dh2048 => "DH+MODP-2048-256",
            KeyAgreement::EcdhP256 => "ECDH+prime256v1-CEUM",
        }
    }

    /// From any of its names.
    pub fn from_name(n: &str) -> Option<Self> {
        match n {
            "DH+MODP-2048-256" | "DHE+MODP-2048-256" => Some(KeyAgreement::Dh2048),
            "ECDH+prime256v1-CEUM" | "ECDHE-CEUM+P256" => Some(KeyAgreement::EcdhP256),
            _ => None,
        }
    }
}

/// The 2048-bit MODP group with a 256-bit prime order subgroup (RFC 5114,
/// 2.3): p, g, q.
const DH_P: &str = "87A8E61DB4B6663CFFBBD19C651959998CEEF608660DD0F25D2CEED4435E3B00E00DF8F1D61957D4FAF7DF4561B2AA3016C3D91134096FAA3BF4296D830E9A7C209E0C6497517ABD5A8A9D306BCF67ED91F9E6725B4758C022E0B1EF4275BF7B6C5BFC11D45F9088B941F54EB1E59BB8BC39A0BF12307F5C4FDB70C581B23F76B63ACAE1CAA6B7902D52526735488A0EF13C6D9A51BFA4AB3AD8347796524D8EF6A167B5A41825D967E144E5140564251CCACB83E6B486F6B3CA3F7971506026C0B857F689962856DED4010ABD0BE621C3A3960A54E710C375F26375D7014103A4B54330C198AF126116D2276E11715F693877FAD7EF09CADB094AE91E1A1597";
const DH_G: &str = "3FB32C9B73134D0B2E77506660EDBD484CA7B18F21EF205407F4793A1A0BA12510DBC15077BE463FFF4FED4AAC0BB555BE3A6C1B0C6B47B1BC3773BF7E8C6F62901228F8C28CBB18A55AE31341000A650196F931C77A57F2DDF463E5E9EC144B777DE62AAAB8A8628AC376D282D6ED3864E67982428EBC831D14348F6F2F9193B5045AF2767164E1DFC967C1FB3F2E55A4BD1BFFE83B9C80D052B985D182EA0ADB2A3B7313D3FE14C8484B1E052588B9B7D2BBD2DF016199ECD06E1557CD0915B3353BBB64E0EC377FD028370DF92B52C7891428CDC67EB6184B523D1DB246C32F63078490F00EF8D647D148D47954515E2327CFEF98C582664B4C0F6CC41659";
const DH_Q: &str = "8CF83642A709A097B447997640129DA299B1A47D1EB3750BA308B0FE64F5FBD3";

fn hex_big(h: &str) -> BigUint {
    BigUint::parse_bytes(h.as_bytes(), 16).expect("a hex constant")
}

/// An ephemeral key agreement key.
enum Ephemeral {
    Dh { x: BigUint },
    Ecdh(p256::ecdh::EphemeralSecret),
}

impl std::fmt::Debug for Ephemeral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ephemeral")
    }
}

impl Ephemeral {
    /// A new key and its public value as sent in `dh1` or `dh2` (8.3).
    fn new(k: KeyAgreement) -> (Self, Vec<u8>) {
        match k {
            KeyAgreement::EcdhP256 => {
                let s = p256::ecdh::EphemeralSecret::random(&mut OsRng);
                let p = p256::EncodedPoint::from(s.public_key());
                (Ephemeral::Ecdh(s), p.as_bytes().to_vec())
            }
            KeyAgreement::Dh2048 => {
                let q = hex_big(DH_Q);
                let x = loop {
                    let mut b = [0u8; 32];
                    OsRng.fill_bytes(&mut b);
                    let x = BigUint::from_bytes_be(&b);
                    if x > BigUint::from(1u8) && x < q {
                        break x;
                    }
                };
                let y = hex_big(DH_G).modpow(&x, &hex_big(DH_P));
                (Ephemeral::Dh { x }, y.to_bytes_be())
            }
        }
    }

    /// SharedSecret: the SHA-256 of the agreed secret (10.3.2.5.2).
    fn agree(&self, peer: &[u8]) -> Result<[u8; 32], Error> {
        match self {
            Ephemeral::Ecdh(s) => {
                let p = p256::PublicKey::from_sec1_bytes(peer)
                    .map_err(|_| Error::new("the peer's ECDH key is not on P-256"))?;
                let z = s.diffie_hellman(&p);
                Ok(Sha256::digest(z.raw_secret_bytes()).into())
            }
            Ephemeral::Dh { x } => {
                let p = hex_big(DH_P);
                let y = BigUint::from_bytes_be(peer);
                let one = BigUint::from(1u8);
                if y <= one || y >= &p - &one || y.modpow(&hex_big(DH_Q), &p) != one {
                    return Err(Error::new("the peer's DH key is not in the group"));
                }
                // The secret as an octet string of the length of p.
                let z = y.modpow(x, &p).to_bytes_be();
                let mut padded = vec![0u8; 256 - z.len()];
                padded.extend_from_slice(&z);
                Ok(Sha256::digest(&padded).into())
            }
        }
    }
}

/// A 256-bit nonce.
fn challenge() -> Vec<u8> {
    let mut c = vec![0; 32];
    OsRng.fill_bytes(&mut c);
    c
}

/// A new AuthRequestMessageToken (10.3.2.4).
pub fn auth_request_token() -> DataHolder {
    DataHolder::new(AUTH_REQUEST_CLASS)
        .with_binary(BinaryProperty::new("future_challenge", challenge()))
}

/// What a finished handshake gives (get_shared_secret and
/// get_authenticated_peer_credential_token).
#[derive(Clone)]
pub struct Authenticated {
    /// SharedSecret.
    pub shared_secret: [u8; 32],
    /// Challenge1, of the request.
    pub challenge1: Vec<u8>,
    /// Challenge2, of the reply.
    pub challenge2: Vec<u8>,
    /// The peer's identity certificate.
    pub peer_cert: Cert,
    /// The peer's permissions document (`c.perm`), as sent.
    pub peer_permissions: Vec<u8>,
    /// The peer's participant data (`c.pdata`), as sent.
    pub peer_pdata: Vec<u8>,
}

impl std::fmt::Debug for Authenticated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticated")
            .field("peer", &self.peer_cert.subject())
            .finish_non_exhaustive()
    }
}

impl Authenticated {
    /// The AuthenticatedPeerCredentialToken (10.3.2.3).
    pub fn peer_credential_token(&self) -> DataHolder {
        DataHolder::new("DDS:Auth:PKI-DH:1.0")
            .with_binary(BinaryProperty::new(
                "c.id",
                self.peer_cert.pem().into_bytes(),
            ))
            .with_binary(BinaryProperty::new("c.perm", self.peer_permissions.clone()))
    }
}

/// One side of a handshake in progress.
#[derive(Debug)]
pub struct Handshake {
    ephemeral: Ephemeral,
    /// The token this side sent last (to resend if it is lost).
    pub sent: DataHolder,
    hash_c1: Vec<u8>,
    hash_c2: Vec<u8>,
    challenge1: Vec<u8>,
    challenge2: Vec<u8>,
    dh1: Vec<u8>,
    dh2: Vec<u8>,
    /// The peer's certificate, permissions and data, once known.
    peer: Option<(Cert, Vec<u8>, Vec<u8>)>,
    /// The challenge the peer promised in its AuthRequestMessageToken.
    remote_future: Option<Vec<u8>>,
}

/// The `c.` properties of a request or reply, in the order of the table.
fn c_properties(
    id: &Identity,
    permissions: &[u8],
    pdata: &[u8],
    kagree: KeyAgreement,
) -> Vec<BinaryProperty> {
    vec![
        BinaryProperty::new("c.id", nul(id.cert.pem().as_bytes())),
        BinaryProperty::new("c.perm", permissions.to_vec()),
        BinaryProperty::new("c.pdata", pdata.to_vec()),
        // Without a NUL: Fast DDS 2.11 compares the names octet by octet.
        BinaryProperty::new(
            "c.dsign_algo",
            id.key.algorithm().name().as_bytes().to_vec(),
        ),
        BinaryProperty::new("c.kagree_algo", kagree.name().as_bytes().to_vec()),
    ]
}

/// `b` with a NUL at the end, as the PEM text of a certificate is sent.
fn nul(b: &[u8]) -> Vec<u8> {
    let mut v = b.to_vec();
    if v.last() != Some(&0) {
        v.push(0);
    }
    v
}

/// The `c.` properties of a received token, in its order.
fn received_c(t: &DataHolder) -> Result<Vec<BinaryProperty>, Error> {
    ["c.id", "c.perm", "c.pdata", "c.dsign_algo", "c.kagree_algo"]
        .iter()
        .map(|n| {
            t.binary(n)
                .map(|v| BinaryProperty::new(n, v.to_vec()))
                .ok_or_else(|| Error::new(format!("the handshake message has no {n}")))
        })
        .collect()
}

fn need<'a>(t: &'a DataHolder, n: &str) -> Result<&'a [u8], Error> {
    t.binary(n)
        .ok_or_else(|| Error::new(format!("the handshake message has no {n}")))
}

/// The participant GUID prefix in `c.pdata`, the CDR of a
/// ParticipantBuiltinTopicData: the value of PID_PARTICIPANT_GUID.
pub fn pdata_prefix(pdata: &[u8]) -> Result<[u8; 12], Error> {
    // An encapsulation header may lead: PL_CDR_BE or PL_CDR_LE.
    let (mut b, little) = match pdata {
        [0, 2, _, _, rest @ ..] => (rest, false),
        [0, 3, _, _, rest @ ..] => (rest, true),
        _ => (pdata, false),
    };
    while b.len() >= 4 {
        let (id, len) = if little {
            (
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[2], b[3]]),
            )
        } else {
            (
                u16::from_be_bytes([b[0], b[1]]),
                u16::from_be_bytes([b[2], b[3]]),
            )
        };
        let len = len as usize;
        if id == 0x0001 || b.len() < 4 + len {
            break;
        }
        if id == 0x0050 && len >= 12 {
            return Ok(b[4..16].try_into().expect("12"));
        }
        b = &b[4 + len..];
    }
    Err(Error::new("c.pdata has no participant GUID"))
}

impl Handshake {
    /// begin_handshake_request: the HandshakeRequestMessageToken.
    /// `future_challenge` is the one of the AuthRequestMessageToken this
    /// side sent, if any.
    pub fn request(
        id: &Identity,
        permissions: &[u8],
        pdata: &[u8],
        future_challenge: Option<&[u8]>,
    ) -> Self {
        let (ephemeral, dh1) = Ephemeral::new(id.key_agreement);
        let c = c_properties(id, permissions, pdata, id.key_agreement);
        let hash_c1 = Sha256::digest(binary_properties_cdr(&c)).to_vec();
        let challenge1 = future_challenge
            .map(<[u8]>::to_vec)
            .unwrap_or_else(challenge);
        let mut t = DataHolder::new(REQUEST_CLASS);
        t.binary_properties = c;
        let t = t
            .with_binary(BinaryProperty::new("hash_c1", hash_c1.clone()))
            .with_binary(BinaryProperty::new("dh1", dh1.clone()))
            .with_binary(BinaryProperty::new("challenge1", challenge1.clone()));
        Handshake {
            ephemeral,
            sent: t,
            hash_c1,
            hash_c2: Vec::new(),
            challenge1,
            challenge2: Vec::new(),
            dh1,
            dh2: Vec::new(),
            peer: None,
            remote_future: None,
        }
    }

    /// begin_handshake_reply: checks the request and gives the
    /// HandshakeReplyMessageToken. `remote_future` is the challenge of
    /// the peer's AuthRequestMessageToken, `future_challenge` this
    /// side's.
    pub fn reply(
        id: &Identity,
        permissions: &[u8],
        pdata: &[u8],
        request: &DataHolder,
        remote_future: Option<&[u8]>,
        future_challenge: Option<&[u8]>,
    ) -> Result<Self, Error> {
        if request.class_id != REQUEST_CLASS {
            return Err(Error::new(format!(
                "not a handshake request: {}",
                request.class_id
            )));
        }
        let c1 = received_c(request)?;
        let challenge1 = need(request, "challenge1")?.to_vec();
        if remote_future.is_some_and(|f| f != challenge1) {
            return Err(Error::new(
                "challenge1 is not the promised future_challenge",
            ));
        }
        let dh1 = need(request, "dh1")?.to_vec();
        let hash_c1 = Sha256::digest(binary_properties_cdr(&c1)).to_vec();
        if request.binary("hash_c1").is_some_and(|h| h != hash_c1) {
            return Err(Error::new("hash_c1 does not match"));
        }
        let peer_cert = id.verify_peer(need(request, "c.id")?)?;
        let pdata1 = need(request, "c.pdata")?.to_vec();
        check_prefix(&pdata_prefix(&pdata1)?, &peer_cert)?;
        let kagree = request
            .binary_str("c.kagree_algo")
            .and_then(KeyAgreement::from_name)
            .ok_or_else(|| Error::new("unknown key agreement"))?;
        let (ephemeral, dh2) = Ephemeral::new(kagree);
        let c2 = c_properties(id, permissions, pdata, kagree);
        let hash_c2 = Sha256::digest(binary_properties_cdr(&c2)).to_vec();
        let challenge2 = future_challenge
            .map(<[u8]>::to_vec)
            .unwrap_or_else(challenge);
        let signed = binary_properties_cdr(&[
            BinaryProperty::new("hash_c2", hash_c2.clone()),
            BinaryProperty::new("challenge2", challenge2.clone()),
            BinaryProperty::new("dh2", dh2.clone()),
            BinaryProperty::new("challenge1", challenge1.clone()),
            BinaryProperty::new("dh1", dh1.clone()),
            BinaryProperty::new("hash_c1", hash_c1.clone()),
        ]);
        let signature = id.key.sign(&signed)?;
        let mut t = DataHolder::new(REPLY_CLASS);
        t.binary_properties = c2;
        let t = t
            .with_binary(BinaryProperty::new("hash_c2", hash_c2.clone()))
            .with_binary(BinaryProperty::new("dh2", dh2.clone()))
            .with_binary(BinaryProperty::new("hash_c1", hash_c1.clone()))
            .with_binary(BinaryProperty::new("dh1", dh1.clone()))
            .with_binary(BinaryProperty::new("challenge1", challenge1.clone()))
            .with_binary(BinaryProperty::new("challenge2", challenge2.clone()))
            .with_binary(BinaryProperty::new("signature", signature));
        let perm1 = need(request, "c.perm")?.to_vec();
        Ok(Handshake {
            ephemeral,
            sent: t,
            hash_c1,
            hash_c2,
            challenge1,
            challenge2,
            dh1,
            dh2,
            peer: Some((peer_cert, perm1, pdata1)),
            remote_future: None,
        })
    }

    /// Notes the challenge the peer promised (for the initiator, checked
    /// against challenge2).
    pub fn expect_future_challenge(&mut self, f: Option<Vec<u8>>) {
        self.remote_future = f;
    }

    /// process_handshake on the initiator: checks the reply; the
    /// HandshakeFinalMessageToken and the result.
    pub fn process_reply(
        &mut self,
        id: &Identity,
        reply: &DataHolder,
    ) -> Result<(DataHolder, Authenticated), Error> {
        if reply.class_id != REPLY_CLASS {
            return Err(Error::new(format!(
                "not a handshake reply: {}",
                reply.class_id
            )));
        }
        if need(reply, "challenge1")? != self.challenge1 {
            return Err(Error::new("the reply is to another request"));
        }
        let challenge2 = need(reply, "challenge2")?.to_vec();
        if self
            .remote_future
            .as_ref()
            .is_some_and(|f| *f != challenge2)
        {
            return Err(Error::new(
                "challenge2 is not the promised future_challenge",
            ));
        }
        let c2 = received_c(reply)?;
        let hash_c2 = Sha256::digest(binary_properties_cdr(&c2)).to_vec();
        if reply.binary("hash_c2").is_some_and(|h| h != hash_c2) {
            return Err(Error::new("hash_c2 does not match"));
        }
        let peer_cert = id.verify_peer(need(reply, "c.id")?)?;
        let pdata2 = need(reply, "c.pdata")?.to_vec();
        check_prefix(&pdata_prefix(&pdata2)?, &peer_cert)?;
        let dh2 = need(reply, "dh2")?.to_vec();
        let signed = binary_properties_cdr(&[
            BinaryProperty::new("hash_c2", hash_c2.clone()),
            BinaryProperty::new("challenge2", challenge2.clone()),
            BinaryProperty::new("dh2", dh2.clone()),
            BinaryProperty::new("challenge1", self.challenge1.clone()),
            BinaryProperty::new("dh1", self.dh1.clone()),
            BinaryProperty::new("hash_c1", self.hash_c1.clone()),
        ]);
        peer_cert
            .public_key()
            .verify(&signed, need(reply, "signature")?)?;
        let shared_secret = self.ephemeral.agree(&dh2)?;
        let final_signed = binary_properties_cdr(&[
            BinaryProperty::new("hash_c1", self.hash_c1.clone()),
            BinaryProperty::new("challenge1", self.challenge1.clone()),
            BinaryProperty::new("dh1", self.dh1.clone()),
            BinaryProperty::new("challenge2", challenge2.clone()),
            BinaryProperty::new("dh2", dh2.clone()),
            BinaryProperty::new("hash_c2", hash_c2.clone()),
        ]);
        let signature = id.key.sign(&final_signed)?;
        let t = DataHolder::new(FINAL_CLASS)
            .with_binary(BinaryProperty::new("hash_c1", self.hash_c1.clone()))
            .with_binary(BinaryProperty::new("hash_c2", hash_c2.clone()))
            .with_binary(BinaryProperty::new("dh1", self.dh1.clone()))
            .with_binary(BinaryProperty::new("dh2", dh2.clone()))
            .with_binary(BinaryProperty::new("challenge1", self.challenge1.clone()))
            .with_binary(BinaryProperty::new("challenge2", challenge2.clone()))
            .with_binary(BinaryProperty::new("signature", signature));
        self.sent = t.clone();
        Ok((
            t,
            Authenticated {
                shared_secret,
                challenge1: self.challenge1.clone(),
                challenge2,
                peer_cert,
                peer_permissions: need(reply, "c.perm")?.to_vec(),
                peer_pdata: pdata2,
            },
        ))
    }

    /// process_handshake on the replier: checks the final message.
    pub fn process_final(&self, fin: &DataHolder) -> Result<Authenticated, Error> {
        if fin.class_id != FINAL_CLASS {
            return Err(Error::new(format!(
                "not a handshake final: {}",
                fin.class_id
            )));
        }
        if need(fin, "challenge1")? != self.challenge1
            || need(fin, "challenge2")? != self.challenge2
        {
            return Err(Error::new("the final message is of another handshake"));
        }
        if fin.binary("hash_c1").is_some_and(|h| h != self.hash_c1) {
            return Err(Error::new("hash_c1 does not match"));
        }
        let (peer_cert, perm, pdata) = self
            .peer
            .clone()
            .ok_or_else(|| Error::new("not a replier"))?;
        let signed = binary_properties_cdr(&[
            BinaryProperty::new("hash_c1", self.hash_c1.clone()),
            BinaryProperty::new("challenge1", self.challenge1.clone()),
            BinaryProperty::new("dh1", self.dh1.clone()),
            BinaryProperty::new("challenge2", self.challenge2.clone()),
            BinaryProperty::new("dh2", self.dh2.clone()),
            BinaryProperty::new("hash_c2", self.hash_c2.clone()),
        ]);
        peer_cert
            .public_key()
            .verify(&signed, need(fin, "signature")?)?;
        Ok(Authenticated {
            shared_secret: self.ephemeral.agree(&self.dh1)?,
            challenge1: self.challenge1.clone(),
            challenge2: self.challenge2.clone(),
            peer_cert,
            peer_permissions: perm,
            peer_pdata: pdata,
        })
    }
}

/// Which side starts the handshake (10.3.4): the participant with the
/// lower GUID.
pub fn initiates(local: &[u8; 16], remote: &[u8; 16]) -> bool {
    local < remote
}
