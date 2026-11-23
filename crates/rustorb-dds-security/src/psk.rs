//! Pre-shared key (PSK) protection of RTPS messages (DDS Security 1.2):
//! the passphrases (10.5.1.3, 10.5.1.4), the key material derived from
//! them (10.5.2.1.3), and the transform of whole RTPS messages with it
//! (encode_rtps_message and decode_rtps_message with
//! `transform_with_psk`, 10.5.3.3.4.6).
//!
//! The same transform serves the builtin DDS:Crypto:AES-GCM-GMAC, when the
//! governance asks for `rtps_psk_protection_kind`, and the PSK plugins
//! DDS:Auth:PSK, DDS:Access:PSK and DDS:Crypto:PSK (10.7 to 10.9), whose
//! tokens are here too.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::Error;
use crate::access::ProtectionKind;
use crate::crypto::{
    Decoder, Encoder, KeyMaterial, TransformKind, header_of, message_prefix_flags, prefix_flag,
    read_submessage, submessage_id,
};
use crate::pki::read_uri;
use crate::token::DataHolder;

/// The `class_id` of the IdentityToken of DDS:Auth:PSK (10.7.2.1).
pub const AUTH_PSK_CLASS: &str = "DDS:Auth:PSK:1.2";
/// The `class_id` of the PermissionsToken of DDS:Access:PSK (10.8.2.2).
pub const ACCESS_PSK_CLASS: &str = "DDS:Access:PSK:1.2";

/// The property of the passphrase (10.5.1, 10.9.1).
pub const SECRET_PASSPHRASE: &str = "dds.sec.crypto.rtps_psk_secret_passphrase";
/// The property of the passphrases also accepted when decoding: the name
/// of 10.5.1. [`SECRET_PASSPHRASE_ALT`] is the name of 10.9.1.
pub const SECRET_PASSPHRASE_EXTRA: &str = "dds.sec.crypto.rtps_psk_secret_passphrase_extra";
/// [`SECRET_PASSPHRASE_EXTRA`] as 10.9.1 names it.
pub const SECRET_PASSPHRASE_ALT: &str = "dds.sec.crypto.rtps_psk_secret_passphrase_alt";
/// The property of the algorithms (10.5.1.2, 10.9.1).
pub const SYMMETRIC_CIPHER_ALGORITHM: &str = "dds.sec.crypto.rtps_psk_symmetric_cipher_algorithm";
/// The default algorithms of the builtin crypto plugin (10.5.1.1), which
/// [`SYMMETRIC_CIPHER_ALGORITHM`] overrides for PSK protection.
pub const DEFAULT_SYMMETRIC_CIPHER_ALGORITHM: &str = "dds.sec.crypto.symmetric_cipher_algorithm";
/// The property of the protection kind of DDS:Access:PSK (10.8.1).
pub const PROTECTION_KIND: &str = "dds.sec.access.rtps_psk_protection_kind";

/// The pre-shared key protection of RTPS messages of a participant
/// (`is_rtps_psk_protected`, 9.4.2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PskConfig {
    /// [`SECRET_PASSPHRASE`]: the URI (`data:,` or `file:`) of
    /// `<passphrase_id>:<passphrase>`.
    pub secret_passphrase: String,
    /// [`SECRET_PASSPHRASE_EXTRA`]: the URI of more passphrases, one a
    /// line, also accepted when decoding.
    pub secret_passphrase_extra: Option<String>,
    /// [`SYMMETRIC_CIPHER_ALGORITHM`]: `AUTO` (the default, AES256-GCM),
    /// `AES128-GCM` or `AES256-GCM`.
    pub symmetric_cipher_algorithm: Option<String>,
    /// [`PROTECTION_KIND`]: NONE, SIGN or ENCRYPT (the default). It is the
    /// configuration of DDS:Access:PSK; with the builtin access control
    /// plugin, the governance's `rtps_psk_protection_kind` decides
    /// instead.
    pub protection_kind: Option<ProtectionKind>,
}

impl PskConfig {
    /// Whether `name` is one of the properties of PSK protection.
    pub fn is_property(name: &str) -> bool {
        [
            SECRET_PASSPHRASE,
            SECRET_PASSPHRASE_EXTRA,
            SECRET_PASSPHRASE_ALT,
            SYMMETRIC_CIPHER_ALGORITHM,
            PROTECTION_KIND,
            // The name Table 76 uses.
            "dds.sec.crypto.psk.secret_passphrase",
        ]
        .contains(&name)
    }

    /// The configuration of the properties: `Ok(None)` without a
    /// passphrase. `dds.sec.crypto.psk.secret_passphrase`, the name
    /// Table 76 uses, is taken too.
    pub fn from_properties<'a>(
        props: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Option<Self>, Error> {
        let props: HashMap<&str, &str> = props.into_iter().collect();
        let Some(secret) = props
            .get(SECRET_PASSPHRASE)
            .or_else(|| props.get("dds.sec.crypto.psk.secret_passphrase"))
        else {
            return Ok(None);
        };
        let protection_kind = match props.get(PROTECTION_KIND) {
            Some(v) => Some(basic_protection_kind(v)?),
            None => None,
        };
        let c = PskConfig {
            secret_passphrase: secret.to_string(),
            secret_passphrase_extra: props
                .get(SECRET_PASSPHRASE_EXTRA)
                .or_else(|| props.get(SECRET_PASSPHRASE_ALT))
                .map(|v| v.to_string()),
            symmetric_cipher_algorithm: props
                .get(SYMMETRIC_CIPHER_ALGORITHM)
                .or_else(|| props.get(DEFAULT_SYMMETRIC_CIPHER_ALGORITHM))
                .map(|v| v.to_string()),
            protection_kind,
        };
        // The algorithm is checked now, the passphrases when they are
        // read.
        cipher_key_len(c.symmetric_cipher_algorithm.as_deref())?;
        Ok(Some(c))
    }
}

/// A BasicProtectionKind: NONE, SIGN or ENCRYPT.
pub fn basic_protection_kind(s: &str) -> Result<ProtectionKind, Error> {
    match ProtectionKind::parse(s)? {
        k @ (ProtectionKind::None | ProtectionKind::Sign | ProtectionKind::Encrypt) => Ok(k),
        k => Err(Error::new(format!(
            "{k:?} is not NONE, SIGN or ENCRYPT for PSK protection"
        ))),
    }
}

/// The key length of a symmetric cipher algorithm pair (Table 22): `AUTO`
/// or nothing is AES256-GCM.
fn cipher_key_len(name: Option<&str>) -> Result<usize, Error> {
    match name.map(str::trim) {
        None | Some("AUTO") | Some("AES256-GCM") => Ok(32),
        Some("AES128-GCM") => Ok(16),
        Some(s) => Err(Error::new(format!(
            "unknown symmetric cipher algorithm {s} (AUTO, AES128-GCM or AES256-GCM)"
        ))),
    }
}

/// One passphrase: its id and the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Passphrase {
    /// passphrase_id.
    pub id: u32,
    /// The passphrase.
    pub secret: String,
}

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Passphrase")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Passphrase {
    /// From `<passphrase_id>:<passphrase>` (10.5.1): the id a decimal
    /// number below 2^32 whose low octet is not 0xFF, the passphrase 1 to
    /// 512 printable ASCII characters that neither start nor end with a
    /// space.
    pub fn parse(s: &str) -> Result<Self, Error> {
        let (id, secret) = s
            .split_once(':')
            .ok_or_else(|| Error::new("a passphrase is <passphrase_id>:<passphrase>"))?;
        let id: u32 = id
            .parse()
            .map_err(|_| Error::new(format!("passphrase_id {id:?} is not a decimal number")))?;
        if id & 0xFF == 0xFF {
            return Err(Error::new(format!(
                "passphrase_id {id}: the ids with a low octet of 0xFF are reserved"
            )));
        }
        if secret.is_empty()
            || secret.len() > 512
            || !secret.bytes().all(|b| (32..=126).contains(&b))
            || secret.starts_with(' ')
            || secret.ends_with(' ')
        {
            return Err(Error::new(
                "a passphrase is 1 to 512 printable ASCII characters, not starting or ending with a space",
            ));
        }
        Ok(Passphrase {
            id,
            secret: secret.into(),
        })
    }

    /// The passphrases at `uri`, one a line (LF, CR or both between
    /// them). `data:` with or without the `,` after it, or `file:`.
    pub fn read_all(uri: &str) -> Result<Vec<Self>, Error> {
        let b = match uri.strip_prefix("data:") {
            Some(d) => d.strip_prefix(',').unwrap_or(d).as_bytes().to_vec(),
            None => read_uri(uri)?,
        };
        let s = String::from_utf8(b).map_err(|_| Error::new("a passphrase is ASCII"))?;
        let all = s
            .split(['\n', '\r'])
            .filter(|l| !l.is_empty())
            .map(Self::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if all.is_empty() {
            return Err(Error::new(format!("no passphrase at {uri}")));
        }
        Ok(all)
    }

    /// The one passphrase at `uri`.
    pub fn read(uri: &str) -> Result<Self, Error> {
        let mut all = Self::read_all(uri)?;
        if all.len() != 1 {
            return Err(Error::new(format!("{uri} has more than one passphrase")));
        }
        Ok(all.remove(0))
    }

    /// PassphraseKeyRevision (Table 76): the three high octets of the id.
    pub fn key_revision(&self) -> [u8; 3] {
        let b = self.id.to_be_bytes();
        [b[0], b[1], b[2]]
    }

    /// SenderKeyId (Table 76): three octets of the SHA-256 of the domain
    /// id and tag, then PassphraseKeyId (the low octet of the id).
    pub fn sender_key_id(&self, domain_id: u32, domain_tag: &str) -> [u8; 4] {
        let h = Sha256::new()
            .chain_update(b"DomainId=")
            .chain_update(domain_id.to_string().as_bytes())
            .chain_update(b";DomainTag=")
            .chain_update(domain_tag.as_bytes())
            .finalize();
        [h[0], h[1], h[2], self.id as u8]
    }

    /// The key material of RTPS messages whose header (protocol version,
    /// vendor id and GUID prefix, the 16 octets after `RTPS`) is
    /// `header` (10.5.2.1.3, Table 75).
    pub fn key_material(
        &self,
        kind: TransformKind,
        domain_id: u32,
        domain_tag: &str,
        header: &[u8; 16],
    ) -> KeyMaterial {
        let key_id = self.sender_key_id(domain_id, domain_tag);
        let derive = |label: &[u8], info: &[u8]| -> Vec<u8> {
            // HMACsha256(HMACsha256(label | SenderKeyId | "RTPS" |
            // ProtocolVersion | VendorId | GuidPrefix, PreSharedSecret),
            // info | 0x01): the first argument is the key.
            let mut salt = label.to_vec();
            salt.extend_from_slice(&key_id);
            salt.extend_from_slice(b"RTPS");
            salt.extend_from_slice(header);
            let prk = hmac(&salt, self.secret.as_bytes());
            let mut k = hmac(&prk, &[info, &[1]].concat()).to_vec();
            k.truncate(kind.key_len());
            k
        };
        KeyMaterial {
            kind,
            revision: self.key_revision(),
            master_salt: derive(b"PSK-SALT", b"master salt derivation"),
            sender_key_id: key_id,
            master_sender_key: derive(b"PSK-SKEY", b"master sender key derivation"),
            receiver_specific_key_id: [0; 4],
            master_receiver_specific_key: Vec::new(),
        }
    }
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

/// The IdentityToken of DDS:Auth:PSK (10.7.2.1).
pub fn identity_token() -> DataHolder {
    DataHolder::new(AUTH_PSK_CLASS)
}

/// The PermissionsToken of DDS:Access:PSK (10.8.2.2).
pub fn permissions_token() -> DataHolder {
    DataHolder::new(ACCESS_PSK_CLASS)
}

/// What a remote PSK key material is found by: the header of the
/// sender's messages, the key id and the key revision.
type DecoderKey = ([u8; 16], [u8; 4], [u8; 3]);

/// How many remote key materials a [`PskProtection`] keeps.
const KEPT_DECODERS: usize = 256;

/// The PSK protection of the RTPS messages of one participant: encodes
/// the messages it sends and decodes those it receives, with key
/// material derived from the passphrase and each message's header.
pub struct PskProtection {
    kind: TransformKind,
    domain_id: u32,
    domain_tag: String,
    own: Passphrase,
    /// The passphrases accepted when decoding: the own first.
    accepted: Vec<Passphrase>,
    /// The encoder, for the header it was derived for.
    encoder: Mutex<Option<([u8; 16], Arc<Encoder>)>>,
    /// The decoders of the senders, by header, key id and revision.
    decoders: Mutex<HashMap<DecoderKey, Arc<Decoder>>>,
}

impl std::fmt::Debug for PskProtection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PskProtection")
            .field("kind", &self.kind)
            .field("passphrase_id", &self.own.id)
            .finish_non_exhaustive()
    }
}

impl PskProtection {
    /// The protection `kind` (SIGN or ENCRYPT) of the messages of a
    /// participant of `domain_id` and `domain_tag`, as `config` says
    /// (its own protection kind is left to the caller).
    pub fn new(
        config: &PskConfig,
        kind: ProtectionKind,
        domain_id: u32,
        domain_tag: &str,
    ) -> Result<Self, Error> {
        let len = cipher_key_len(config.symmetric_cipher_algorithm.as_deref())?;
        let kind = match (kind, len) {
            (ProtectionKind::Encrypt, 16) => TransformKind::Aes128Gcm,
            (ProtectionKind::Encrypt, _) => TransformKind::Aes256Gcm,
            (ProtectionKind::Sign, 16) => TransformKind::Aes128Gmac,
            (ProtectionKind::Sign, _) => TransformKind::Aes256Gmac,
            (k, _) => {
                return Err(Error::new(format!(
                    "PSK protection is SIGN or ENCRYPT, not {k:?}"
                )));
            }
        };
        let own = Passphrase::read(&config.secret_passphrase)?;
        let mut accepted = vec![own.clone()];
        if let Some(extra) = &config.secret_passphrase_extra {
            accepted.extend(Passphrase::read_all(extra)?);
        }
        Ok(PskProtection {
            kind,
            domain_id,
            domain_tag: domain_tag.into(),
            own,
            accepted,
            encoder: Mutex::new(None),
            decoders: Mutex::new(HashMap::new()),
        })
    }

    /// The transformation of the messages: AES-GCM or AES-GMAC.
    pub fn transform_kind(&self) -> TransformKind {
        self.kind
    }

    /// Whether `message` is protected with a pre-shared key (the
    /// PreSharedKeyFlag of its SRTPS_PREFIX).
    pub fn is_psk_protected(message: &[u8]) -> bool {
        message_prefix_flags(message).is_some_and(|f| f & prefix_flag::PSK != 0)
    }

    /// encode_rtps_message with `transform_with_psk`: the header (and
    /// the original header extension, which rustorb-dds does not send)
    /// unchanged, then SRTPS_PREFIX with the A and P flags, SEC_BODY (or
    /// the submessages), SRTPS_POSTFIX.
    pub fn encode(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        if message.len() < 20 || &message[..4] != b"RTPS" {
            return Err(Error::new("not an RTPS message"));
        }
        let header: [u8; 16] = message[4..20].try_into().expect("16");
        let enc = {
            let mut e = self.encoder.lock().unwrap_or_else(|e| e.into_inner());
            match &*e {
                Some((h, enc)) if *h == header => enc.clone(),
                _ => {
                    let km =
                        self.own
                            .key_material(self.kind, self.domain_id, &self.domain_tag, &header);
                    let enc = Arc::new(Encoder::new(km));
                    *e = Some((header, enc.clone()));
                    enc
                }
            }
        };
        enc.encode_message_with(message, &[], prefix_flag::AAD | prefix_flag::PSK)
    }

    /// decode_rtps_message of a message with the PreSharedKeyFlag: checks
    /// that the transformation is the expected one, finds the passphrase
    /// by the key id and revision, and verifies (and decrypts) with the
    /// key material derived for the sender's header. The header must be
    /// authenticated (the A flag), as DDS:Crypto:PSK asks.
    pub fn decode(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        if message.len() < 20 || &message[..4] != b"RTPS" {
            return Err(Error::new("not an RTPS message"));
        }
        let (prefix, _) = read_submessage(&message[20..])?;
        if prefix.id != submessage_id::SRTPS_PREFIX || prefix.flags & prefix_flag::PSK == 0 {
            return Err(Error::new("not protected with a pre-shared key"));
        }
        if prefix.flags & prefix_flag::AAD == 0 {
            return Err(Error::new(
                "a message protected with a pre-shared key must authenticate its header",
            ));
        }
        let h = header_of(prefix.body)?;
        if TransformKind::from_octets(h.kind)? != self.kind {
            return Err(Error::new(format!(
                "the message is protected with {:?}, not {:?}",
                TransformKind::from_octets(h.kind)?,
                self.kind
            )));
        }
        let revision = [h.kind[0], h.kind[1], h.kind[2]];
        let header: [u8; 16] = message[4..20].try_into().expect("16");
        let key = (header, h.key_id, revision);
        let found = self
            .decoders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        let dec = match found {
            Some(d) => d,
            None => {
                let p = self
                    .accepted
                    .iter()
                    .find(|p| {
                        p.key_revision() == revision
                            && p.sender_key_id(self.domain_id, &self.domain_tag) == h.key_id
                    })
                    .ok_or_else(|| Error::new("no passphrase of this key id and revision"))?;
                let d = Arc::new(Decoder::new(p.key_material(
                    self.kind,
                    self.domain_id,
                    &self.domain_tag,
                    &header,
                )));
                let mut ds = self.decoders.lock().unwrap_or_else(|e| e.into_inner());
                if ds.len() >= KEPT_DECODERS {
                    ds.clear();
                }
                ds.insert(key, d.clone());
                d
            }
        };
        dec.decode_message(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    const EXAMPLE: &str = "data:,5632:castle super radar denial swing lunar kind swarm wet toilet output harbor basic begin margin huge year visit";

    /// The example of 10.5.2.1.3.
    #[test]
    fn derives_the_key_material_of_the_example() {
        let p = Passphrase::read(EXAMPLE).unwrap();
        assert_eq!(p.id, 5632);
        assert_eq!(p.key_revision(), [0, 0, 0x16]);
        assert_eq!(p.sender_key_id(201, ""), [0x61, 0xC8, 0x53, 0x00]);
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(&[0x02, 0x05, 0x01, 0x01]);
        header[4..].copy_from_slice(&hex("DFCD91E1686804516CB1B60E"));
        let k = p.key_material(TransformKind::Aes256Gcm, 201, "", &header);
        assert_eq!(
            k.master_salt,
            hex("a4ebff5738dc6826c8d3f5e55a24bb96d9e80147b51c4e49a0927c4fa2cfec8c")
        );
        assert_eq!(
            k.master_sender_key,
            hex("4708460adc6bb886521fbdc4b3a9d34e27eed36c162ccdf4fb7427a7f347738b")
        );
        assert_eq!(k.kind_octets(), [0, 0, 0x16, 4]);
        // AES128 takes the first 128 bits.
        let k = p.key_material(TransformKind::Aes128Gmac, 201, "", &header);
        assert_eq!(k.master_sender_key, hex("4708460adc6bb886521fbdc4b3a9d34e"));
    }

    #[test]
    fn passphrases_are_checked() {
        assert!(Passphrase::parse("12:Open Sesame").is_ok());
        assert!(Passphrase::parse("255:x").is_err());
        assert!(Passphrase::parse("4294967296:x").is_err());
        assert!(Passphrase::parse("1: x").is_err());
        assert!(Passphrase::parse("1:x ").is_err());
        assert!(Passphrase::parse("1:").is_err());
        assert!(Passphrase::parse("1:caf\u{e9}").is_err());
        assert!(Passphrase::parse(&format!("1:{}", "a".repeat(513))).is_err());
        assert!(Passphrase::parse("x:y").is_err());
        // data: with or without the comma; several, one a line.
        assert_eq!(Passphrase::read("data:5612:Open Sesame").unwrap().id, 5612);
        let all = Passphrase::read_all("data:,5613:A\n5614:B\r\n5615:C").unwrap();
        assert_eq!(
            all.iter().map(|p| p.id).collect::<Vec<_>>(),
            [5613, 5614, 5615]
        );
        assert!(Passphrase::read("data:,1:a\n2:b").is_err());
    }

    fn config(uri: &str) -> PskConfig {
        PskConfig {
            secret_passphrase: uri.into(),
            secret_passphrase_extra: None,
            symmetric_cipher_algorithm: None,
            protection_kind: None,
        }
    }

    fn message(prefix: u8) -> Vec<u8> {
        let mut m = b"RTPS\x02\x05\x01\x0f".to_vec();
        m.extend_from_slice(&[prefix; 12]);
        m.extend_from_slice(&[0x09, 0x01, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
        m.extend_from_slice(&[0x15, 0x01, 4, 0, b'h', b'e', b'l', b'o']);
        m
    }

    #[test]
    fn messages_round_trip_between_participants() {
        for kind in [ProtectionKind::Sign, ProtectionKind::Encrypt] {
            for alg in ["AUTO", "AES128-GCM"] {
                let mut c = config("data:,77:shared secret");
                c.symmetric_cipher_algorithm = Some(alg.into());
                let a = PskProtection::new(&c, kind, 3, "").unwrap();
                let b = PskProtection::new(&c, kind, 3, "").unwrap();
                let m = message(1);
                let e = a.encode(&m).unwrap();
                assert!(PskProtection::is_psk_protected(&e));
                assert_eq!(&e[..20], &m[..20]);
                assert_eq!(kind.encrypts(), !e.windows(4).any(|w| w == b"helo"));
                assert_eq!(b.decode(&e).unwrap(), m);
                // From another sender too.
                let m2 = message(2);
                assert_eq!(b.decode(&a.encode(&m2).unwrap()).unwrap(), m2);
                // A changed header does not verify.
                let mut bad = e.clone();
                bad[19] ^= 1;
                assert!(b.decode(&bad).is_err());
            }
        }
    }

    #[test]
    fn other_secrets_domains_and_kinds_are_refused() {
        let a =
            PskProtection::new(&config("data:,77:one"), ProtectionKind::Encrypt, 3, "").unwrap();
        let e = a.encode(&message(1)).unwrap();
        // Another passphrase with the same id.
        let other =
            PskProtection::new(&config("data:,77:two"), ProtectionKind::Encrypt, 3, "").unwrap();
        assert!(other.decode(&e).is_err());
        // Another domain: another sender key id.
        let d4 =
            PskProtection::new(&config("data:,77:one"), ProtectionKind::Encrypt, 4, "").unwrap();
        assert!(d4.decode(&e).is_err());
        // Another domain tag.
        let tag =
            PskProtection::new(&config("data:,77:one"), ProtectionKind::Encrypt, 3, "t").unwrap();
        assert!(tag.decode(&e).is_err());
        // SIGN does not take ENCRYPT.
        let sign =
            PskProtection::new(&config("data:,77:one"), ProtectionKind::Sign, 3, "").unwrap();
        assert!(sign.decode(&e).is_err());
        // Not protected at all.
        assert!(a.decode(&message(1)).is_err());
    }

    #[test]
    fn extra_passphrases_are_accepted_when_decoding() {
        let old =
            PskProtection::new(&config("data:,77:old"), ProtectionKind::Encrypt, 3, "").unwrap();
        let mut c = config("data:,78:new");
        c.secret_passphrase_extra = Some("data:,77:old\n79:other".into());
        let new = PskProtection::new(&c, ProtectionKind::Encrypt, 3, "").unwrap();
        let m = message(1);
        assert_eq!(new.decode(&old.encode(&m).unwrap()).unwrap(), m);
        // The old participant does not take the new passphrase.
        assert!(old.decode(&new.encode(&m).unwrap()).is_err());
    }

    #[test]
    fn configurations_come_from_properties() {
        assert_eq!(PskConfig::from_properties([("x", "y")]).unwrap(), None);
        let c = PskConfig::from_properties([
            (SECRET_PASSPHRASE, "data:,1:a"),
            (SECRET_PASSPHRASE_ALT, "data:,2:b"),
            (PROTECTION_KIND, "SIGN"),
            (SYMMETRIC_CIPHER_ALGORITHM, "AES128-GCM"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(c.secret_passphrase_extra.as_deref(), Some("data:,2:b"));
        assert_eq!(c.protection_kind, Some(ProtectionKind::Sign));
        assert!(
            PskConfig::from_properties([
                (SECRET_PASSPHRASE, "data:,1:a"),
                (PROTECTION_KIND, "ENCRYPT_WITH_ORIGIN_AUTHENTICATION")
            ])
            .is_err()
        );
        assert!(
            PskConfig::from_properties([
                (SECRET_PASSPHRASE, "data:,1:a"),
                (SYMMETRIC_CIPHER_ALGORITHM, "DES")
            ])
            .is_err()
        );
        let c = PskConfig::from_properties([("dds.sec.crypto.psk.secret_passphrase", "data:,1:a")])
            .unwrap()
            .unwrap();
        assert_eq!(c.secret_passphrase, "data:,1:a");
    }
}
