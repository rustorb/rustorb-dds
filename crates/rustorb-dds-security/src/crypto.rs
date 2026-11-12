//! The builtin cryptographic plugin, DDS:Crypto:AES-GCM-GMAC (DDS
//! Security 1.2, 10.5): key material, session keys, and the transforms of
//! serialized payloads, submessages and whole RTPS messages.
//!
//! The functions here work on octets: a payload, one submessage or one
//! message as it goes on the wire. Which key material to use for which
//! remote endpoint is the caller's business.

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce, Tag};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::Error;
use crate::cdr::{CdrReader, CdrWriter};

/// Submessage ids of the secure submessages (7.3.7).
pub mod submessage_id {
    /// SEC_BODY.
    pub const SEC_BODY: u8 = 0x30;
    /// SEC_PREFIX.
    pub const SEC_PREFIX: u8 = 0x31;
    /// SEC_POSTFIX.
    pub const SEC_POSTFIX: u8 = 0x32;
    /// SRTPS_PREFIX.
    pub const SRTPS_PREFIX: u8 = 0x33;
    /// SRTPS_POSTFIX.
    pub const SRTPS_POSTFIX: u8 = 0x34;
}
use submessage_id::*;

const INFO_SRC: u8 = 0x0c;
const RTPS_HEADER: usize = 20;

/// The `class_id` of the plugin's CryptoTokens (10.5.2.1).
pub const CRYPTO_TOKEN_CLASS: &str = "DDS:Crypto:AES_GCM_GMAC";
/// The binary property of a CryptoToken holding the key material.
pub const KEYMAT_PROPERTY: &str = "dds.cryp.keymat";

/// CryptoTransformKind (10.5.2.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformKind {
    /// No encryption, no authentication tag.
    None,
    /// AES128-GMAC.
    Aes128Gmac,
    /// AES128-GCM.
    Aes128Gcm,
    /// AES256-GMAC.
    Aes256Gmac,
    /// AES256-GCM.
    Aes256Gcm,
}

impl TransformKind {
    /// The four octets on the wire.
    pub fn octets(self) -> [u8; 4] {
        [
            0,
            0,
            0,
            match self {
                TransformKind::None => 0,
                TransformKind::Aes128Gmac => 1,
                TransformKind::Aes128Gcm => 2,
                TransformKind::Aes256Gmac => 3,
                TransformKind::Aes256Gcm => 4,
            },
        ]
    }

    /// From the four octets on the wire.
    pub fn from_octets(o: [u8; 4]) -> Result<Self, Error> {
        Ok(match o {
            [0, 0, 0, 0] => TransformKind::None,
            [0, 0, 0, 1] => TransformKind::Aes128Gmac,
            [0, 0, 0, 2] => TransformKind::Aes128Gcm,
            [0, 0, 0, 3] => TransformKind::Aes256Gmac,
            [0, 0, 0, 4] => TransformKind::Aes256Gcm,
            _ => return Err(Error::new(format!("unknown transformation kind {o:?}"))),
        })
    }

    /// Whether the transform encrypts (GCM) rather than only signs (GMAC).
    pub fn encrypts(self) -> bool {
        matches!(self, TransformKind::Aes128Gcm | TransformKind::Aes256Gcm)
    }

    /// The length of the keys, in octets (0 for NONE).
    pub fn key_len(self) -> usize {
        match self {
            TransformKind::None => 0,
            TransformKind::Aes128Gmac | TransformKind::Aes128Gcm => 16,
            TransformKind::Aes256Gmac | TransformKind::Aes256Gcm => 32,
        }
    }
}

/// KeyMaterial_AES_GCM_GMAC (10.5.2.1.1).
#[derive(Clone, PartialEq, Eq)]
pub struct KeyMaterial {
    /// transformation_kind.
    pub kind: TransformKind,
    /// master_salt.
    pub master_salt: Vec<u8>,
    /// sender_key_id.
    pub sender_key_id: [u8; 4],
    /// master_sender_key.
    pub master_sender_key: Vec<u8>,
    /// receiver_specific_key_id; zero for none.
    pub receiver_specific_key_id: [u8; 4],
    /// master_receiver_specific_key; empty for none.
    pub master_receiver_specific_key: Vec<u8>,
}

impl std::fmt::Debug for KeyMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyMaterial")
            .field("kind", &self.kind)
            .field("sender_key_id", &self.sender_key_id)
            .field("receiver_specific_key_id", &self.receiver_specific_key_id)
            .finish_non_exhaustive()
    }
}

/// A fresh key id: distinct within this process.
pub fn new_key_id() -> [u8; 4] {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    static BASE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let base = *BASE.get_or_init(|| OsRng.next_u32());
    let mut id = base.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed));
    if id == 0 {
        id = base.wrapping_add(NEXT.fetch_add(1, Ordering::Relaxed));
    }
    id.to_be_bytes()
}

fn random(n: usize) -> Vec<u8> {
    let mut v = vec![0; n];
    OsRng.fill_bytes(&mut v);
    v
}

impl KeyMaterial {
    /// New key material of `kind`, with no receiver-specific key.
    pub fn generate(kind: TransformKind) -> Self {
        let n = kind.key_len();
        KeyMaterial {
            kind,
            master_salt: random(n),
            sender_key_id: if n == 0 { [0; 4] } else { new_key_id() },
            master_sender_key: random(n),
            receiver_specific_key_id: [0; 4],
            master_receiver_specific_key: Vec::new(),
        }
    }

    /// A copy with a new receiver-specific key, for one receiver (the
    /// Participant2Participant, Writer2Reader and Reader2Writer key
    /// material).
    pub fn with_receiver_specific_key(&self) -> Self {
        let mut k = self.clone();
        if self.kind != TransformKind::None {
            k.receiver_specific_key_id = new_key_id();
            k.master_receiver_specific_key = random(self.kind.key_len());
        }
        k
    }

    /// The key material of the volatile message secure endpoints, derived
    /// from the shared secret of a handshake (10.5.2.1.2).
    pub fn key_exchange(challenge1: &[u8], challenge2: &[u8], shared_secret: &[u8]) -> Self {
        let salt_in = Sha256::new()
            .chain_update(challenge1)
            .chain_update(b"keyexchange salt")
            .chain_update(challenge2)
            .finalize();
        let key_in = Sha256::new()
            .chain_update(challenge2)
            .chain_update(b"key exchange key")
            .chain_update(challenge1)
            .finalize();
        KeyMaterial {
            kind: TransformKind::Aes256Gcm,
            master_salt: hmac256(&salt_in, &[shared_secret]).to_vec(),
            sender_key_id: [0; 4],
            master_sender_key: hmac256(&key_in, &[shared_secret]).to_vec(),
            receiver_specific_key_id: [0; 4],
            master_receiver_specific_key: Vec::new(),
        }
    }

    /// The big-endian CDR of the key material (10.5.2.1).
    pub fn to_cdr(&self) -> Vec<u8> {
        let mut w = CdrWriter::new();
        w.octets(&self.kind.octets());
        w.sequence(&self.master_salt);
        w.octets(&self.sender_key_id);
        w.sequence(&self.master_sender_key);
        w.octets(&self.receiver_specific_key_id);
        w.sequence(&self.master_receiver_specific_key);
        w.into_inner()
    }

    /// From its big-endian CDR.
    pub fn from_cdr(b: &[u8]) -> Result<Self, Error> {
        let mut r = CdrReader::new(b);
        let kind = TransformKind::from_octets(r.array()?)?;
        let k = KeyMaterial {
            kind,
            master_salt: r.sequence(32)?,
            sender_key_id: r.array()?,
            master_sender_key: r.sequence(32)?,
            receiver_specific_key_id: r.array()?,
            master_receiver_specific_key: r.sequence(32)?,
        };
        let n = kind.key_len();
        let rs = k.master_receiver_specific_key.len();
        if k.master_sender_key.len() != n
            || k.master_salt.len() < n
            || (rs != 0 && rs != n)
            || ((k.receiver_specific_key_id == [0; 4]) != (rs == 0))
        {
            return Err(Error::new("inconsistent key material"));
        }
        Ok(k)
    }
}

fn hmac256(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    for d in data {
        m.update(d);
    }
    m.finalize().into_bytes().into()
}

/// SessionKey or SessionReceiverSpecificKey (10.5.3.3.3).
fn session_key(
    kind: TransformKind,
    master: &[u8],
    salt: &[u8],
    label: &[u8],
    session: [u8; 4],
) -> Vec<u8> {
    let mut k = hmac256(master, &[label, salt, &session]).to_vec();
    k.truncate(kind.key_len());
    k
}

#[derive(Clone)]
enum Cipher {
    A128(Box<Aes128Gcm>),
    A256(Box<Aes256Gcm>),
}

impl Cipher {
    fn new(key: &[u8]) -> Cipher {
        if key.len() == 16 {
            Cipher::A128(Box::new(Aes128Gcm::new_from_slice(key).expect("16 octets")))
        } else {
            Cipher::A256(Box::new(Aes256Gcm::new_from_slice(key).expect("32 octets")))
        }
    }

    fn seal(&self, iv: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> [u8; 16] {
        let n = &Nonce::from(*iv);
        let t = match self {
            Cipher::A128(c) => c.encrypt_in_place_detached(n, aad, buf),
            Cipher::A256(c) => c.encrypt_in_place_detached(n, aad, buf),
        };
        t.expect("lengths within limits").into()
    }

    fn open(&self, iv: &[u8; 12], aad: &[u8], buf: &mut [u8], tag: &[u8; 16]) -> Result<(), Error> {
        let n = &Nonce::from(*iv);
        let t = &Tag::from(*tag);
        match self {
            Cipher::A128(c) => c.decrypt_in_place_detached(n, aad, buf, t),
            Cipher::A256(c) => c.decrypt_in_place_detached(n, aad, buf, t),
        }
        .map_err(|_| Error::new("the authentication tag does not verify"))
    }

    /// AES-GMAC: GCM with nothing to encrypt and `data` authenticated.
    fn mac(&self, iv: &[u8; 12], data: &[u8]) -> [u8; 16] {
        self.seal(iv, data, &mut [])
    }
}

/// The CryptoHeader (10.5.2.3): 20 octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CryptoHeader {
    /// transformation_kind.
    pub kind: [u8; 4],
    /// transformation_key_id.
    pub key_id: [u8; 4],
    /// session_id.
    pub session_id: [u8; 4],
    /// initialization_vector_suffix.
    pub iv_suffix: [u8; 8],
}

impl CryptoHeader {
    /// Its length on the wire.
    pub const LEN: usize = 20;

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.kind);
        out.extend_from_slice(&self.key_id);
        out.extend_from_slice(&self.session_id);
        out.extend_from_slice(&self.iv_suffix);
    }

    /// From its octets.
    pub fn read(b: &[u8]) -> Result<Self, Error> {
        if b.len() < Self::LEN {
            return Err(Error::new("a CryptoHeader is 20 octets"));
        }
        let a = |i: usize| -> [u8; 4] { b[i..i + 4].try_into().expect("4") };
        Ok(CryptoHeader {
            kind: a(0),
            key_id: a(4),
            session_id: a(8),
            iv_suffix: b[12..20].try_into().expect("8"),
        })
    }

    fn iv(&self) -> [u8; 12] {
        let mut iv = [0; 12];
        iv[..4].copy_from_slice(&self.session_id);
        iv[4..].copy_from_slice(&self.iv_suffix);
        iv
    }
}

/// The CryptoFooter (10.5.2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CryptoFooter {
    /// common_mac.
    pub common_mac: [u8; 16],
    /// receiver_specific_macs: key id and MAC.
    pub receiver_specific_macs: Vec<([u8; 4], [u8; 16])>,
}

impl CryptoFooter {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.common_mac);
        out.extend_from_slice(&(self.receiver_specific_macs.len() as u32).to_be_bytes());
        for (id, mac) in &self.receiver_specific_macs {
            out.extend_from_slice(id);
            out.extend_from_slice(mac);
        }
    }

    fn len(&self) -> usize {
        20 + 20 * self.receiver_specific_macs.len()
    }

    /// From its octets; the rest after it.
    pub fn read(b: &[u8]) -> Result<(Self, &[u8]), Error> {
        if b.len() < 20 {
            return Err(Error::new("the CryptoFooter is cut short"));
        }
        let common_mac = b[..16].try_into().expect("16");
        let n = u32::from_be_bytes(b[16..20].try_into().expect("4")) as usize;
        let rest = &b[20..];
        if n > rest.len() / 20 {
            return Err(Error::new("the CryptoFooter is cut short"));
        }
        let receiver_specific_macs = (0..n)
            .map(|i| {
                let e = &rest[i * 20..i * 20 + 20];
                (
                    e[..4].try_into().expect("4"),
                    e[4..].try_into().expect("16"),
                )
            })
            .collect();
        Ok((
            CryptoFooter {
                common_mac,
                receiver_specific_macs,
            },
            &rest[n * 20..],
        ))
    }
}

/// The default limit of AES blocks under one session key.
pub const MAX_BLOCKS_PER_SESSION: u64 = 1 << 32;

/// The sending side of one key material: it keeps the session and the
/// initialization vector (10.5.3.3.2).
#[derive(Debug)]
pub struct Encoder {
    key: KeyMaterial,
    max_blocks: u64,
    state: Mutex<Session>,
}

struct Session {
    id: u32,
    iv: u64,
    blocks: u64,
    cipher: Option<Cipher>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// What one transform used: the header and the cipher of the session.
struct Use {
    header: CryptoHeader,
    cipher: Option<Cipher>,
}

impl Encoder {
    /// An encoder of `key`.
    pub fn new(key: KeyMaterial) -> Self {
        Encoder {
            key,
            max_blocks: MAX_BLOCKS_PER_SESSION,
            state: Mutex::new(Session {
                id: OsRng.next_u32(),
                iv: OsRng.next_u64(),
                blocks: 0,
                cipher: None,
            }),
        }
    }

    /// The key material.
    pub fn key_material(&self) -> &KeyMaterial {
        &self.key
    }

    /// Takes the next initialization vector for `len` octets.
    fn next(&self, len: usize) -> Use {
        let k = &self.key;
        let mut s = self.state.lock().expect("not poisoned");
        let blocks = (len as u64).div_ceil(16) + 1;
        if s.cipher.is_none() || s.blocks + blocks > self.max_blocks {
            if s.cipher.is_some() {
                s.id = s.id.wrapping_add(1);
            }
            s.blocks = 0;
            s.cipher = (k.kind != TransformKind::None).then(|| {
                Cipher::new(&session_key(
                    k.kind,
                    &k.master_sender_key,
                    &k.master_salt,
                    b"SessionKey",
                    s.id.to_be_bytes(),
                ))
            });
        }
        s.blocks += blocks;
        s.iv = s.iv.wrapping_add(1);
        Use {
            header: CryptoHeader {
                kind: k.kind.octets(),
                key_id: k.sender_key_id,
                session_id: s.id.to_be_bytes(),
                iv_suffix: s.iv.to_be_bytes(),
            },
            cipher: s.cipher.clone(),
        }
    }

    /// Encrypts or signs `plain` in place; the footer with the MACs of
    /// each receiver's specific key.
    fn seal(
        &self,
        u: &Use,
        aad: &[u8],
        data: &mut [u8],
        receivers: &[&KeyMaterial],
    ) -> CryptoFooter {
        let iv = u.header.iv();
        let common_mac = match &u.cipher {
            None => [0; 16],
            Some(c) if self.key.kind.encrypts() => c.seal(&iv, aad, data),
            Some(c) => {
                if aad.is_empty() {
                    c.mac(&iv, data)
                } else {
                    let mut all = aad.to_vec();
                    all.extend_from_slice(data);
                    c.mac(&iv, &all)
                }
            }
        };
        let receiver_specific_macs = receivers
            .iter()
            .filter(|r| r.receiver_specific_key_id != [0; 4] && r.kind != TransformKind::None)
            .map(|r| {
                let key = session_key(
                    r.kind,
                    &r.master_receiver_specific_key,
                    &r.master_salt,
                    b"SessionReceiverKey",
                    u.header.session_id,
                );
                (
                    r.receiver_specific_key_id,
                    Cipher::new(&key).mac(&iv, &common_mac),
                )
            })
            .collect();
        CryptoFooter {
            common_mac,
            receiver_specific_macs,
        }
    }

    /// encode_serialized_payload (10.5.3.3.4.4): CryptoHeader, then the
    /// CryptoContent (GCM) or the payload unchanged (GMAC), then the
    /// CryptoFooter with no receiver-specific MACs.
    pub fn encode_payload(&self, payload: &[u8]) -> Vec<u8> {
        let u = self.next(payload.len());
        let mut out = Vec::with_capacity(payload.len() + 64);
        u.header.write(&mut out);
        let mut data = payload.to_vec();
        let footer = self.seal(&u, &[], &mut data, &[]);
        if self.key.kind.encrypts() {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        out.extend_from_slice(&data);
        footer.write(&mut out);
        out
    }

    /// encode_datawriter_submessage and encode_datareader_submessage
    /// (10.5.3.3.4.5): SEC_PREFIX, then SEC_BODY (GCM) or the submessage
    /// unchanged (GMAC), then SEC_POSTFIX. `submessage` is a whole
    /// submessage, header included; `receivers` give the receiver-specific
    /// keys.
    pub fn encode_submessage(&self, submessage: &[u8], receivers: &[&KeyMaterial]) -> Vec<u8> {
        let u = self.next(submessage.len());
        let mut out = Vec::with_capacity(submessage.len() + 96);
        let mut h = Vec::with_capacity(20);
        u.header.write(&mut h);
        put_submessage(&mut out, SEC_PREFIX, 0, &h);
        let mut data = submessage.to_vec();
        let footer = self.seal(&u, &[], &mut data, receivers);
        if self.key.kind.encrypts() {
            let mut body = (data.len() as u32).to_be_bytes().to_vec();
            body.extend_from_slice(&data);
            put_submessage(&mut out, SEC_BODY, 0, &body);
        } else {
            out.extend_from_slice(&data);
        }
        let mut f = Vec::with_capacity(footer.len());
        footer.write(&mut f);
        put_submessage(&mut out, SEC_POSTFIX, 0, &f);
        out
    }

    /// encode_rtps_message (10.5.3.3.4.6, AAD not enabled): the header
    /// unchanged, SRTPS_PREFIX, then SEC_BODY (GCM) or the body, then
    /// SRTPS_POSTFIX. The body is the message with its header turned into
    /// an INFO_SRC.
    pub fn encode_message(
        &self,
        message: &[u8],
        receivers: &[&KeyMaterial],
    ) -> Result<Vec<u8>, Error> {
        if message.len() < RTPS_HEADER || &message[..4] != b"RTPS" {
            return Err(Error::new("not an RTPS message"));
        }
        let mut body = Vec::with_capacity(message.len() + 8);
        put_submessage(
            &mut body,
            INFO_SRC,
            0,
            &info_source(&message[..RTPS_HEADER]),
        );
        body.extend_from_slice(&message[RTPS_HEADER..]);
        let u = self.next(body.len());
        let mut out = Vec::with_capacity(body.len() + 128);
        out.extend_from_slice(&message[..RTPS_HEADER]);
        let mut h = Vec::with_capacity(20);
        u.header.write(&mut h);
        put_submessage(&mut out, SRTPS_PREFIX, 0, &h);
        let footer = self.seal(&u, &[], &mut body, receivers);
        if self.key.kind.encrypts() {
            let mut b = (body.len() as u32).to_be_bytes().to_vec();
            b.extend_from_slice(&body);
            put_submessage(&mut out, SEC_BODY, 0, &b);
        } else {
            out.extend_from_slice(&body);
        }
        let mut f = Vec::with_capacity(footer.len());
        footer.write(&mut f);
        put_submessage(&mut out, SRTPS_POSTFIX, 0, &f);
        Ok(out)
    }
}

/// The INFO_SRC body equivalent to an RTPS header: unused, version,
/// vendor, prefix.
fn info_source(header: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 4];
    b.extend_from_slice(&header[4..RTPS_HEADER]);
    b
}

/// Appends a little-endian submessage of `body`.
fn put_submessage(out: &mut Vec<u8>, id: u8, flags: u8, body: &[u8]) {
    out.push(id);
    out.push(flags | 1);
    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.extend_from_slice(body);
}

/// One submessage read off the wire: its id, flags, and body.
#[derive(Clone, Copy, Debug)]
pub struct RawSubmessage<'a> {
    /// The submessage id.
    pub id: u8,
    /// The flags.
    pub flags: u8,
    /// The body.
    pub body: &'a [u8],
    /// The whole submessage, header included.
    pub whole: &'a [u8],
}

/// Reads the submessage at the start of `b`; it and the rest.
pub fn read_submessage(b: &[u8]) -> Result<(RawSubmessage<'_>, &[u8]), Error> {
    if b.len() < 4 {
        return Err(Error::new("a submessage is cut short"));
    }
    let len = if b[1] & 1 == 1 {
        u16::from_le_bytes([b[2], b[3]])
    } else {
        u16::from_be_bytes([b[2], b[3]])
    } as usize;
    // octetsToNextHeader 0 means "to the end" only for the last
    // submessage (DDSI-RTPS 9.4.5.1.3); none of ours is that.
    let len = if len == 0 && !matches!(b[0], 0x01 | 0x09) {
        b.len() - 4
    } else {
        len
    };
    if b.len() < 4 + len {
        return Err(Error::new("a submessage is cut short"));
    }
    Ok((
        RawSubmessage {
            id: b[0],
            flags: b[1],
            body: &b[4..4 + len],
            whole: &b[..4 + len],
        },
        &b[4 + len..],
    ))
}

/// The receiving side of one remote key material.
#[derive(Debug)]
pub struct Decoder {
    key: KeyMaterial,
    sessions: Mutex<HashMap<[u8; 4], Keys>>,
}

#[derive(Clone)]
struct Keys {
    sender: Cipher,
    receiver: Option<Cipher>,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys")
    }
}

/// What a decoded secure submessage or message contains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The plain submessage, payload or message.
    pub plain: Vec<u8>,
    /// The octets after the secure postfix.
    pub rest_offset: usize,
}

impl Decoder {
    /// A decoder of the remote `key`: its receiver-specific key is the
    /// one this side checks.
    pub fn new(key: KeyMaterial) -> Self {
        Decoder {
            key,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// The key material.
    pub fn key_material(&self) -> &KeyMaterial {
        &self.key
    }

    /// Whether `header` was made with this key material.
    pub fn matches(&self, header: &CryptoHeader) -> bool {
        header.key_id == self.key.sender_key_id && header.kind == self.key.kind.octets()
    }

    fn keys(&self, header: &CryptoHeader) -> Result<Option<Keys>, Error> {
        if !self.matches(header) {
            return Err(Error::new("unknown key material"));
        }
        let k = &self.key;
        if k.kind == TransformKind::None {
            return Ok(None);
        }
        let mut s = self.sessions.lock().expect("not poisoned");
        if s.len() > 16 {
            s.clear();
        }
        Ok(Some(
            s.entry(header.session_id)
                .or_insert_with(|| Keys {
                    sender: Cipher::new(&session_key(
                        k.kind,
                        &k.master_sender_key,
                        &k.master_salt,
                        b"SessionKey",
                        header.session_id,
                    )),
                    receiver: (k.receiver_specific_key_id != [0; 4]).then(|| {
                        Cipher::new(&session_key(
                            k.kind,
                            &k.master_receiver_specific_key,
                            &k.master_salt,
                            b"SessionReceiverKey",
                            header.session_id,
                        ))
                    }),
                })
                .clone(),
        ))
    }

    /// Checks the MACs of `footer` and decrypts `data` in place.
    fn open(
        &self,
        header: &CryptoHeader,
        data: &mut [u8],
        footer: &CryptoFooter,
    ) -> Result<(), Error> {
        let Some(keys) = self.keys(header)? else {
            return Ok(());
        };
        let iv = header.iv();
        if self.key.kind.encrypts() {
            keys.sender.open(&iv, &[], data, &footer.common_mac)?;
        } else if keys.sender.mac(&iv, data) != footer.common_mac {
            return Err(Error::new("the common MAC does not verify"));
        }
        if let Some(r) = &keys.receiver {
            let id = self.key.receiver_specific_key_id;
            let (_, mac) = footer
                .receiver_specific_macs
                .iter()
                .find(|(i, _)| *i == id)
                .ok_or_else(|| Error::new("no receiver-specific MAC for this receiver"))?;
            if r.mac(&iv, &footer.common_mac) != *mac {
                return Err(Error::new("the receiver-specific MAC does not verify"));
            }
        }
        Ok(())
    }

    /// decode_serialized_payload: the plain payload of what
    /// [`Encoder::encode_payload`] made.
    pub fn decode_payload(&self, b: &[u8]) -> Result<Vec<u8>, Error> {
        let header = CryptoHeader::read(b)?;
        let rest = &b[CryptoHeader::LEN..];
        let (mut data, rest) = if self.key.kind.encrypts() {
            if rest.len() < 4 {
                return Err(Error::new("the CryptoContent is cut short"));
            }
            let n = u32::from_be_bytes(rest[..4].try_into().expect("4")) as usize;
            if rest.len() < 4 + n {
                return Err(Error::new("the CryptoContent is cut short"));
            }
            (rest[4..4 + n].to_vec(), &rest[4 + n..])
        } else {
            // The footer has no receiver-specific MACs: 20 octets.
            if rest.len() < 20 {
                return Err(Error::new("the CryptoFooter is cut short"));
            }
            let n = rest.len() - 20;
            (rest[..n].to_vec(), &rest[n..])
        };
        let (footer, _) = CryptoFooter::read(rest)?;
        self.open(&header, &mut data, &footer)?;
        Ok(data)
    }

    /// decode_datawriter_submessage and decode_datareader_submessage: from
    /// the SEC_PREFIX at the start of `b`, the plain submessage, and how
    /// many octets of `b` the secure submessages took.
    pub fn decode_submessage(&self, b: &[u8]) -> Result<Decoded, Error> {
        let (prefix, rest) = read_submessage(b)?;
        if prefix.id != SEC_PREFIX {
            return Err(Error::new("not a SEC_PREFIX"));
        }
        let header = CryptoHeader::read(prefix.body)?;
        let (inner, rest) = read_submessage(rest)?;
        let mut data = if self.key.kind.encrypts() {
            if inner.id != SEC_BODY {
                return Err(Error::new("no SEC_BODY after the SEC_PREFIX"));
            }
            content(inner.body)?.to_vec()
        } else {
            inner.whole.to_vec()
        };
        let (postfix, rest) = read_submessage(rest)?;
        if postfix.id != SEC_POSTFIX {
            return Err(Error::new("no SEC_POSTFIX"));
        }
        let (footer, _) = CryptoFooter::read(postfix.body)?;
        self.open(&header, &mut data, &footer)?;
        Ok(Decoded {
            plain: data,
            rest_offset: b.len() - rest.len(),
        })
    }

    /// decode_rtps_message: the plain message of what
    /// [`Encoder::encode_message`] made.
    pub fn decode_message(&self, b: &[u8]) -> Result<Vec<u8>, Error> {
        if b.len() < RTPS_HEADER || &b[..4] != b"RTPS" {
            return Err(Error::new("not an RTPS message"));
        }
        let (prefix, rest) = read_submessage(&b[RTPS_HEADER..])?;
        if prefix.id != SRTPS_PREFIX {
            return Err(Error::new("not an SRTPS_PREFIX"));
        }
        let header = CryptoHeader::read(prefix.body)?;
        let (mut body, footer) = if self.key.kind.encrypts() {
            let (inner, rest) = read_submessage(rest)?;
            if inner.id != SEC_BODY {
                return Err(Error::new("no SEC_BODY after the SRTPS_PREFIX"));
            }
            let (postfix, _) = read_submessage(rest)?;
            (content(inner.body)?.to_vec(), postfix)
        } else {
            // The submessages up to the SRTPS_POSTFIX.
            let mut at = rest;
            let mut n = 0;
            loop {
                let (s, r) = read_submessage(at)?;
                if s.id == SRTPS_POSTFIX {
                    break (rest[..n].to_vec(), s);
                }
                n += s.whole.len();
                at = r;
            }
        };
        if footer.id != SRTPS_POSTFIX {
            return Err(Error::new("no SRTPS_POSTFIX"));
        }
        let (f, _) = CryptoFooter::read(footer.body)?;
        self.open(&header, &mut body, &f)?;
        // The INFO_SRC back into a header.
        let (src, rest) = read_submessage(&body)?;
        if src.id != INFO_SRC || src.body.len() != 20 {
            return Err(Error::new(
                "the secure body does not start with an INFO_SRC",
            ));
        }
        let mut out = Vec::with_capacity(body.len());
        out.extend_from_slice(&b[..4]);
        out.extend_from_slice(&src.body[4..]);
        out.extend_from_slice(rest);
        Ok(out)
    }
}

/// The octets of a CryptoContent.
fn content(b: &[u8]) -> Result<&[u8], Error> {
    if b.len() < 4 {
        return Err(Error::new("the CryptoContent is cut short"));
    }
    let n = u32::from_be_bytes(b[..4].try_into().expect("4")) as usize;
    b.get(4..4 + n)
        .ok_or_else(|| Error::new("the CryptoContent is cut short"))
}

/// The CryptoHeader at the start of a SEC_PREFIX or SRTPS_PREFIX body, or
/// of an encoded payload: which key material to decode with.
pub fn header_of(b: &[u8]) -> Result<CryptoHeader, Error> {
    CryptoHeader::read(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [TransformKind; 4] = [
        TransformKind::Aes128Gmac,
        TransformKind::Aes128Gcm,
        TransformKind::Aes256Gmac,
        TransformKind::Aes256Gcm,
    ];

    #[test]
    fn key_material_round_trips() {
        for kind in KINDS {
            let k = KeyMaterial::generate(kind).with_receiver_specific_key();
            let b = k.to_cdr();
            assert_eq!(KeyMaterial::from_cdr(&b).unwrap(), k);
        }
        let none = KeyMaterial::generate(TransformKind::None);
        assert_eq!(none.to_cdr().len(), 4 + 4 + 4 + 4 + 4 + 4);
    }

    #[test]
    fn payloads_round_trip_and_are_checked() {
        for kind in KINDS {
            let k = KeyMaterial::generate(kind);
            let e = Encoder::new(k.clone());
            let d = Decoder::new(k);
            let p = b"\x00\x01\x00\x00hello, world".to_vec();
            let c = e.encode_payload(&p);
            assert_eq!(d.decode_payload(&c).unwrap(), p);
            assert_eq!(kind.encrypts(), !c.windows(5).any(|w| w == b"hello"));
            let mut bad = c.clone();
            bad[30] ^= 1;
            assert!(d.decode_payload(&bad).is_err());
        }
    }

    #[test]
    fn submessages_round_trip_with_receiver_specific_macs() {
        for kind in KINDS {
            let local = KeyMaterial::generate(kind);
            let r1 = local.with_receiver_specific_key();
            let r2 = local.with_receiver_specific_key();
            let e = Encoder::new(local);
            let sub = [0x15u8, 0x05, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8];
            let mut c = e.encode_submessage(&sub, &[&r1, &r2]);
            c.extend_from_slice(b"tail");
            for r in [&r1, &r2] {
                let d = Decoder::new(r.clone()).decode_submessage(&c).unwrap();
                assert_eq!(d.plain, sub);
                assert_eq!(&c[d.rest_offset..], b"tail");
            }
            // A receiver with another specific key is refused.
            let other = Decoder::new(r1.with_receiver_specific_key());
            assert!(other.decode_submessage(&c).is_err());
        }
    }

    #[test]
    fn messages_round_trip() {
        for kind in KINDS {
            let k = KeyMaterial::generate(kind).with_receiver_specific_key();
            let e = Encoder::new(k.clone());
            let mut m = b"RTPS\x02\x05\x01\x0f".to_vec();
            m.extend_from_slice(&[7; 12]);
            m.extend_from_slice(&[0x09, 0x01, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
            m.extend_from_slice(&[0x07, 0x01, 4, 0, 9, 9, 9, 9]);
            let c = e.encode_message(&m, &[&k]).unwrap();
            assert_eq!(&c[..20], &m[..20]);
            assert_eq!(Decoder::new(k).decode_message(&c).unwrap(), m);
        }
    }

    #[test]
    fn sessions_change_after_the_block_limit() {
        let k = KeyMaterial::generate(TransformKind::Aes128Gcm);
        let mut e = Encoder::new(k.clone());
        e.max_blocks = 4;
        let d = Decoder::new(k);
        let a = e.encode_payload(&[1; 40]);
        let b = e.encode_payload(&[2; 40]);
        assert_ne!(a[8..12], b[8..12]);
        assert_eq!(d.decode_payload(&b).unwrap(), vec![2; 40]);
    }

    #[test]
    fn key_exchange_material_is_symmetric() {
        let a = KeyMaterial::key_exchange(b"c1", b"c2", b"secret");
        let b = KeyMaterial::key_exchange(b"c1", b"c2", b"secret");
        assert_eq!(a, b);
        assert_eq!(a.kind, TransformKind::Aes256Gcm);
        assert_eq!(a.master_sender_key.len(), 32);
        assert_ne!(a.master_sender_key, a.master_salt);
    }
}
