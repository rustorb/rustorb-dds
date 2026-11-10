//! The basic RTPS types and submessage elements
//! (DDSI-RTPS 2.5, 8.3.5, 9.3 and 9.4.2).

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::error::DecodeError;
use crate::wire::{Endianness, Reader, Writer};

/// The 12-octet prefix shared by the GUIDs of all entities of one
/// participant (DDSI-RTPS 2.5, 9.3.1.1).
///
/// The first two octets must be the vendor id of the implementation
/// (9.3.1.5).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GuidPrefix(pub [u8; 12]);

impl GuidPrefix {
    /// `GUIDPREFIX_UNKNOWN`: all zeros.
    pub const UNKNOWN: GuidPrefix = GuidPrefix([0; 12]);

    /// Makes a prefix that is very likely unique in the domain: the vendor
    /// id in the first two octets (9.3.1.5), then ten octets mixed from
    /// the process id, the time, a counter and the standard library's
    /// random hash keys.
    pub fn generate(vendor_id: VendorId) -> GuidPrefix {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut h = RandomState::new().build_hasher();
        h.write_u32(std::process::id());
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            h.write_u128(d.as_nanos());
        }
        let a = h.finish();
        h.write_u64(a);
        let b = h.finish();
        let mut out = [0u8; 12];
        out[..2].copy_from_slice(&vendor_id.0);
        out[2..10].copy_from_slice(&a.to_be_bytes());
        out[10..].copy_from_slice(&b.to_be_bytes()[..2]);
        GuidPrefix(out)
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        r.array().map(GuidPrefix)
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.bytes(&self.0);
    }
}

impl fmt::Debug for GuidPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GuidPrefix(")?;
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                f.write_str(".")?;
            }
            write!(f, "{b:02x}")?;
        }
        f.write_str(")")
    }
}

/// The identifier of an entity within its participant
/// (DDSI-RTPS 2.5, 9.3.1.2).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId {
    /// The three-octet key. Chosen by the implementation unless the entity
    /// is a predefined one.
    pub entity_key: [u8; 3],
    /// The kind of entity; see [`entity_kind`].
    pub entity_kind: u8,
}

/// Values of [`EntityId::entity_kind`] (DDSI-RTPS 2.5, Table 9.1).
///
/// The two most significant bits say who defined the entity: `00` user,
/// `11` built-in, `01` vendor-specific (9.3.1.2).
pub mod entity_kind {
    /// Unknown, user-defined.
    pub const USER_UNKNOWN: u8 = 0x00;
    /// Writer with a key, user-defined.
    pub const USER_WRITER_WITH_KEY: u8 = 0x02;
    /// Writer without a key, user-defined.
    pub const USER_WRITER_NO_KEY: u8 = 0x03;
    /// Reader without a key, user-defined.
    pub const USER_READER_NO_KEY: u8 = 0x04;
    /// Reader with a key, user-defined.
    pub const USER_READER_WITH_KEY: u8 = 0x07;
    /// Writer group, user-defined.
    pub const USER_WRITER_GROUP: u8 = 0x08;
    /// Reader group, user-defined.
    pub const USER_READER_GROUP: u8 = 0x09;
    /// Unknown, built-in.
    pub const BUILTIN_UNKNOWN: u8 = 0xc0;
    /// Participant (always built-in).
    pub const BUILTIN_PARTICIPANT: u8 = 0xc1;
    /// Writer with a key, built-in.
    pub const BUILTIN_WRITER_WITH_KEY: u8 = 0xc2;
    /// Writer without a key, built-in.
    pub const BUILTIN_WRITER_NO_KEY: u8 = 0xc3;
    /// Reader without a key, built-in.
    pub const BUILTIN_READER_NO_KEY: u8 = 0xc4;
    /// Reader with a key, built-in.
    pub const BUILTIN_READER_WITH_KEY: u8 = 0xc7;
    /// Writer group, built-in.
    pub const BUILTIN_WRITER_GROUP: u8 = 0xc8;
    /// Reader group, built-in.
    pub const BUILTIN_READER_GROUP: u8 = 0xc9;
}

impl EntityId {
    const fn new(key: [u8; 3], kind: u8) -> Self {
        EntityId {
            entity_key: key,
            entity_kind: kind,
        }
    }

    /// `ENTITYID_UNKNOWN`.
    pub const UNKNOWN: EntityId = EntityId::new([0, 0, 0], 0x00);
    /// `ENTITYID_PARTICIPANT` (Table 9.2).
    pub const PARTICIPANT: EntityId = EntityId::new([0, 0, 1], 0xc1);
    /// `ENTITYID_SEDP_BUILTIN_TOPICS_ANNOUNCER`.
    pub const SEDP_BUILTIN_TOPICS_ANNOUNCER: EntityId = EntityId::new([0, 0, 2], 0xc2);
    /// `ENTITYID_SEDP_BUILTIN_TOPICS_DETECTOR`.
    pub const SEDP_BUILTIN_TOPICS_DETECTOR: EntityId = EntityId::new([0, 0, 2], 0xc7);
    /// `ENTITYID_SEDP_BUILTIN_PUBLICATIONS_ANNOUNCER`.
    pub const SEDP_BUILTIN_PUBLICATIONS_ANNOUNCER: EntityId = EntityId::new([0, 0, 3], 0xc2);
    /// `ENTITYID_SEDP_BUILTIN_PUBLICATIONS_DETECTOR`.
    pub const SEDP_BUILTIN_PUBLICATIONS_DETECTOR: EntityId = EntityId::new([0, 0, 3], 0xc7);
    /// `ENTITYID_SEDP_BUILTIN_SUBSCRIPTIONS_ANNOUNCER`.
    pub const SEDP_BUILTIN_SUBSCRIPTIONS_ANNOUNCER: EntityId = EntityId::new([0, 0, 4], 0xc2);
    /// `ENTITYID_SEDP_BUILTIN_SUBSCRIPTIONS_DETECTOR`.
    pub const SEDP_BUILTIN_SUBSCRIPTIONS_DETECTOR: EntityId = EntityId::new([0, 0, 4], 0xc7);
    /// `ENTITYID_SPDP_BUILTIN_PARTICIPANT_ANNOUNCER`.
    pub const SPDP_BUILTIN_PARTICIPANT_ANNOUNCER: EntityId = EntityId::new([0, 1, 0], 0xc2);
    /// `ENTITYID_SPDP_BUILTIN_PARTICIPANT_DETECTOR`.
    pub const SPDP_BUILTIN_PARTICIPANT_DETECTOR: EntityId = EntityId::new([0, 1, 0], 0xc7);
    /// `ENTITYID_P2P_BUILTIN_PARTICIPANT_MESSAGE_WRITER`.
    pub const P2P_BUILTIN_PARTICIPANT_MESSAGE_WRITER: EntityId = EntityId::new([0, 2, 0], 0xc2);
    /// `ENTITYID_P2P_BUILTIN_PARTICIPANT_MESSAGE_READER`.
    pub const P2P_BUILTIN_PARTICIPANT_MESSAGE_READER: EntityId = EntityId::new([0, 2, 0], 0xc7);
    /// `ENTITYID_TL_SVC_REQ_WRITER` (DDS-XTypes 1.3 Table 61).
    pub const TL_SVC_REQ_WRITER: EntityId = EntityId::new([0, 3, 0], 0xc3);
    /// `ENTITYID_TL_SVC_REQ_READER`.
    pub const TL_SVC_REQ_READER: EntityId = EntityId::new([0, 3, 0], 0xc4);
    /// `ENTITYID_TL_SVC_REPLY_WRITER`.
    pub const TL_SVC_REPLY_WRITER: EntityId = EntityId::new([0, 3, 1], 0xc3);
    /// `ENTITYID_TL_SVC_REPLY_READER`.
    pub const TL_SVC_REPLY_READER: EntityId = EntityId::new([0, 3, 1], 0xc4);

    /// Makes a user-defined entity id from a key and a kind.
    pub const fn user(entity_key: [u8; 3], entity_kind: u8) -> Self {
        EntityId::new(entity_key, entity_kind)
    }

    /// The four octets as they appear on the wire.
    pub const fn to_bytes(self) -> [u8; 4] {
        let k = self.entity_key;
        [k[0], k[1], k[2], self.entity_kind]
    }

    /// Builds an entity id from its four wire octets.
    pub const fn from_bytes(b: [u8; 4]) -> Self {
        EntityId::new([b[0], b[1], b[2]], b[3])
    }

    /// Whether the kind marks a built-in entity (top bits `11`).
    pub const fn is_builtin(self) -> bool {
        self.entity_kind & 0xc0 == 0xc0
    }

    /// Whether the kind marks a vendor-specific entity (top bits `01`).
    pub const fn is_vendor_specific(self) -> bool {
        self.entity_kind & 0xc0 == 0x40
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        r.array().map(EntityId::from_bytes)
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.bytes(&self.to_bytes());
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d] = self.to_bytes();
        write!(f, "EntityId({a:02x}{b:02x}{c:02x}.{d:02x})")
    }
}

/// A globally unique identifier of an entity (DDSI-RTPS 2.5, 9.3.1.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Guid {
    /// The participant part.
    pub prefix: GuidPrefix,
    /// The entity part.
    pub entity_id: EntityId,
}

impl Guid {
    /// `GUID_UNKNOWN`.
    pub const UNKNOWN: Guid = Guid {
        prefix: GuidPrefix::UNKNOWN,
        entity_id: EntityId::UNKNOWN,
    };

    /// Joins a prefix and an entity id.
    pub const fn new(prefix: GuidPrefix, entity_id: EntityId) -> Self {
        Guid { prefix, entity_id }
    }

    /// The 16 octets: the prefix followed by the entity id.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..12].copy_from_slice(&self.prefix.0);
        out[12..].copy_from_slice(&self.entity_id.to_bytes());
        out
    }

    /// From the 16 octets of [`to_bytes`](Self::to_bytes).
    pub fn from_bytes(b: [u8; 16]) -> Self {
        let mut prefix = [0u8; 12];
        prefix.copy_from_slice(&b[..12]);
        Guid::new(
            GuidPrefix(prefix),
            EntityId::from_bytes([b[12], b[13], b[14], b[15]]),
        )
    }
}

/// The vendor of an RTPS implementation (DDSI-RTPS 2.5, 8.3.3.1.3).
///
/// Values are assigned by the OMG.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VendorId(pub [u8; 2]);

impl VendorId {
    /// `VENDORID_UNKNOWN`.
    pub const UNKNOWN: VendorId = VendorId([0, 0]);

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        r.array().map(VendorId)
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.bytes(&self.0);
    }
}

/// An RTPS protocol version (DDSI-RTPS 2.5, 9.4.2.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolVersion {
    /// Major version.
    pub major: u8,
    /// Minor version.
    pub minor: u8,
}

impl ProtocolVersion {
    /// 1.0
    pub const V1_0: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
    /// 1.1
    pub const V1_1: ProtocolVersion = ProtocolVersion { major: 1, minor: 1 };
    /// 2.0
    pub const V2_0: ProtocolVersion = ProtocolVersion { major: 2, minor: 0 };
    /// 2.1
    pub const V2_1: ProtocolVersion = ProtocolVersion { major: 2, minor: 1 };
    /// 2.2
    pub const V2_2: ProtocolVersion = ProtocolVersion { major: 2, minor: 2 };
    /// 2.3
    pub const V2_3: ProtocolVersion = ProtocolVersion { major: 2, minor: 3 };
    /// 2.4
    pub const V2_4: ProtocolVersion = ProtocolVersion { major: 2, minor: 4 };
    /// 2.5
    pub const V2_5: ProtocolVersion = ProtocolVersion { major: 2, minor: 5 };
    /// `PROTOCOLVERSION`: the version this crate implements, 2.5.
    pub const CURRENT: ProtocolVersion = ProtocolVersion::V2_5;

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let [major, minor] = r.array()?;
        Ok(ProtocolVersion { major, minor })
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.bytes(&[self.major, self.minor]);
    }
}

/// A 64-bit signed sequence number (DDSI-RTPS 2.5, 8.3.5.4 and 9.3.2.10).
///
/// On the wire it is `{ long high; unsigned long low; }` and the value is
/// `high * 2^32 + low`. Sequence numbers start at 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequenceNumber(pub i64);

impl SequenceNumber {
    /// `SEQUENCENUMBER_UNKNOWN`: high = -1, low = 0.
    pub const UNKNOWN: SequenceNumber = SequenceNumber(-(1 << 32));

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let high = r.i32()?;
        let low = r.u32()?;
        Ok(SequenceNumber(((high as i64) << 32) | low as i64))
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.i32((self.0 >> 32) as i32);
        w.u32(self.0 as u32);
    }
}

/// The identity of a sample: its writer and its sequence number
/// (`SampleIdentity`, DDS-RPC 1.0, 7.5.1.1.1). A reply carries that of
/// its request in-line, as `PID_RELATED_SAMPLE_IDENTITY` (7.8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SampleIdentity {
    /// `writer_guid`.
    pub writer_guid: Guid,
    /// `sequence_number`.
    pub sequence_number: SequenceNumber,
}

impl SampleIdentity {
    /// `RELATED_SAMPLE_IDENTITY_INVALID`: `GUID_UNKNOWN` and
    /// `SEQUENCENUMBER_UNKNOWN`.
    pub const INVALID: SampleIdentity = SampleIdentity {
        writer_guid: Guid::UNKNOWN,
        sequence_number: SequenceNumber::UNKNOWN,
    };

    /// The CDR serialization (24 octets) in `endianness`.
    pub fn to_bytes(&self, endianness: Endianness) -> Vec<u8> {
        let mut out = Vec::with_capacity(24);
        let mut w = Writer::new(&mut out, endianness);
        w.bytes(&self.writer_guid.to_bytes());
        self.sequence_number.write(&mut w);
        out
    }

    /// Reads the CDR serialization in `endianness`.
    pub fn from_bytes(bytes: &[u8], endianness: Endianness) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes, endianness);
        let prefix = GuidPrefix::read(&mut r)?;
        let entity_id = EntityId::read(&mut r)?;
        Ok(SampleIdentity {
            writer_guid: Guid::new(prefix, entity_id),
            sequence_number: SequenceNumber::read(&mut r)?,
        })
    }
}

/// A 32-bit fragment number, starting at 1 (DDSI-RTPS 2.5, 8.3.5.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentNumber(pub u32);

/// The bitmap shared by [`SequenceNumberSet`] and [`FragmentNumberSet`]
/// (DDSI-RTPS 2.5, 9.4.2.6): up to 256 bits, the first bit is the most
/// significant bit of the first 32-bit word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Bitmap {
    num_bits: u32,
    words: [u32; 8],
}

impl Bitmap {
    const MAX_BITS: u32 = 256;

    fn new(num_bits: u32) -> Option<Self> {
        (num_bits <= Self::MAX_BITS).then_some(Bitmap {
            num_bits,
            words: [0; 8],
        })
    }

    fn get(&self, offset: u32) -> bool {
        offset < self.num_bits
            && self.words[(offset / 32) as usize] & (1 << (31 - offset % 32)) != 0
    }

    fn set(&mut self, offset: u32) -> bool {
        if offset >= self.num_bits {
            return false;
        }
        self.words[(offset / 32) as usize] |= 1 << (31 - offset % 32);
        true
    }

    fn ones(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.num_bits).filter(|&i| self.get(i))
    }

    fn word_count(&self) -> usize {
        self.num_bits.div_ceil(32) as usize
    }

    /// Reads `numBits` and the words. Bits past `numBits` are cleared, as
    /// they carry no meaning.
    fn read(r: &mut Reader<'_>, invalid: DecodeError) -> Result<Self, DecodeError> {
        let num_bits = r.u32()?;
        let mut bitmap = Bitmap::new(num_bits).ok_or(invalid)?;
        for i in 0..bitmap.word_count() {
            bitmap.words[i] = r.u32()?;
        }
        let rem = num_bits % 32;
        if rem != 0 {
            let last = bitmap.word_count() - 1;
            bitmap.words[last] &= !(u32::MAX >> rem);
        }
        Ok(bitmap)
    }

    fn write(&self, w: &mut Writer<'_>) {
        w.u32(self.num_bits);
        for word in &self.words[..self.word_count()] {
            w.u32(*word);
        }
    }
}

/// A set of up to 256 sequence numbers starting at `base`
/// (DDSI-RTPS 2.5, 8.3.5.5 and 9.4.2.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SequenceNumberSet {
    base: SequenceNumber,
    bitmap: Bitmap,
}

impl SequenceNumberSet {
    /// An empty set covering `base .. base + num_bits`.
    ///
    /// Returns `None` if `base` is less than 1 or `num_bits` is more than
    /// 256.
    pub fn new(base: SequenceNumber, num_bits: u32) -> Option<Self> {
        if base.0 < 1 {
            return None;
        }
        Some(SequenceNumberSet {
            base,
            bitmap: Bitmap::new(num_bits)?,
        })
    }

    /// The first sequence number the set can hold (`bitmapBase`).
    pub fn base(&self) -> SequenceNumber {
        self.base
    }

    /// How many sequence numbers, from `base`, the set covers (`numBits`).
    pub fn num_bits(&self) -> u32 {
        self.bitmap.num_bits
    }

    /// Adds `sn`. Returns false, and leaves the set as it is, when `sn` is
    /// outside `base .. base + num_bits`.
    pub fn insert(&mut self, sn: SequenceNumber) -> bool {
        match sn.0.checked_sub(self.base.0) {
            Some(d) if (0..self.bitmap.num_bits as i64).contains(&d) => self.bitmap.set(d as u32),
            _ => false,
        }
    }

    /// Whether `sn` is in the set.
    pub fn contains(&self, sn: SequenceNumber) -> bool {
        match sn.0.checked_sub(self.base.0) {
            Some(d) if (0..self.bitmap.num_bits as i64).contains(&d) => self.bitmap.get(d as u32),
            _ => false,
        }
    }

    /// The sequence numbers in the set, in increasing order.
    pub fn iter(&self) -> impl Iterator<Item = SequenceNumber> + '_ {
        self.bitmap
            .ones()
            .map(move |d| SequenceNumber(self.base.0 + d as i64))
    }

    pub(crate) fn read(r: &mut Reader<'_>, id: u8) -> Result<Self, DecodeError> {
        let invalid = DecodeError::InvalidSubmessage {
            id,
            reason: "invalid SequenceNumberSet",
        };
        let base = SequenceNumber::read(r)?;
        if base.0 < 1 {
            return Err(invalid);
        }
        let bitmap = Bitmap::read(r, invalid)?;
        Ok(SequenceNumberSet { base, bitmap })
    }

    /// Like `read`, but also takes an empty set with base 0 and returns
    /// it as an empty set with base 1. Base 0 is invalid (8.3.5.5), but
    /// some implementations send it in an ACKNACK to say that nothing was
    /// received yet; "everything before 0" and "everything before 1"
    /// acknowledge the same nothing.
    pub(crate) fn read_allowing_empty_zero(
        r: &mut Reader<'_>,
        id: u8,
    ) -> Result<Self, DecodeError> {
        let invalid = DecodeError::InvalidSubmessage {
            id,
            reason: "invalid SequenceNumberSet",
        };
        let base = SequenceNumber::read(r)?;
        let bitmap = Bitmap::read(r, invalid.clone())?;
        if base.0 >= 1 {
            Ok(SequenceNumberSet { base, bitmap })
        } else if base.0 == 0 && bitmap.num_bits == 0 {
            Ok(SequenceNumberSet {
                base: SequenceNumber(1),
                bitmap,
            })
        } else {
            Err(invalid)
        }
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        self.base.write(w);
        self.bitmap.write(w);
    }
}

/// A set of up to 256 fragment numbers starting at `base`
/// (DDSI-RTPS 2.5, 8.3.5.7 and 9.4.2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FragmentNumberSet {
    base: FragmentNumber,
    bitmap: Bitmap,
}

impl FragmentNumberSet {
    /// An empty set covering `base .. base + num_bits`.
    ///
    /// Returns `None` if `base` is 0 or `num_bits` is more than 256.
    pub fn new(base: FragmentNumber, num_bits: u32) -> Option<Self> {
        if base.0 < 1 {
            return None;
        }
        Some(FragmentNumberSet {
            base,
            bitmap: Bitmap::new(num_bits)?,
        })
    }

    /// The first fragment number the set can hold (`bitmapBase`).
    pub fn base(&self) -> FragmentNumber {
        self.base
    }

    /// How many fragment numbers, from `base`, the set covers (`numBits`).
    pub fn num_bits(&self) -> u32 {
        self.bitmap.num_bits
    }

    /// Adds `n`. Returns false when `n` is outside the range of the set.
    pub fn insert(&mut self, n: FragmentNumber) -> bool {
        match n.0.checked_sub(self.base.0) {
            Some(d) => self.bitmap.set(d),
            None => false,
        }
    }

    /// Whether `n` is in the set.
    pub fn contains(&self, n: FragmentNumber) -> bool {
        match n.0.checked_sub(self.base.0) {
            Some(d) => self.bitmap.get(d),
            None => false,
        }
    }

    /// The fragment numbers in the set, in increasing order.
    pub fn iter(&self) -> impl Iterator<Item = FragmentNumber> + '_ {
        self.bitmap
            .ones()
            .map(move |d| FragmentNumber(self.base.0 + d))
    }

    pub(crate) fn read(r: &mut Reader<'_>, id: u8) -> Result<Self, DecodeError> {
        let invalid = DecodeError::InvalidSubmessage {
            id,
            reason: "invalid FragmentNumberSet",
        };
        let base = FragmentNumber(r.u32()?);
        if base.0 < 1 {
            return Err(invalid);
        }
        let bitmap = Bitmap::read(r, invalid)?;
        Ok(FragmentNumberSet { base, bitmap })
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.u32(self.base.0);
        self.bitmap.write(w);
    }
}

/// A point in time in NTP format: `seconds + fraction / 2^32`
/// (DDSI-RTPS 2.5, 9.3.2.1 and 9.3.2.2).
///
/// The IDL in 9.3.2.1 makes `seconds` an `unsigned long`, and the special
/// values use `0xffffffff`, so it is held as `u32` here. (The drawing in
/// 9.4.2.9 labels it `long`; the bits are the same.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Time {
    /// Whole seconds.
    pub seconds: u32,
    /// Fraction of a second in units of 2^-32 s.
    pub fraction: u32,
}

impl Time {
    /// `TIME_ZERO`.
    pub const ZERO: Time = Time {
        seconds: 0,
        fraction: 0,
    };
    /// `TIME_INVALID`.
    pub const INVALID: Time = Time {
        seconds: 0xffff_ffff,
        fraction: 0xffff_ffff,
    };
    /// `TIME_INFINITE`.
    pub const INFINITE: Time = Time {
        seconds: 0xffff_ffff,
        fraction: 0xffff_fffe,
    };

    /// The current time, counted from the UNIX epoch.
    ///
    /// DDSI-RTPS does not fix the origin of `Time_t`; the UNIX epoch is
    /// assumed here and is to be checked against other implementations
    /// (rustorb-dds#8).
    pub fn now() -> Time {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Time {
            seconds: d.as_secs() as u32,
            fraction: ((u64::from(d.subsec_nanos()) << 32) / 1_000_000_000) as u32,
        }
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Time {
            seconds: r.u32()?,
            fraction: r.u32()?,
        })
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.u32(self.seconds);
        w.u32(self.fraction);
    }
}

/// A counter used to detect duplicate submessages
/// (DDSI-RTPS 2.5, 8.3.5.10 and 9.4.2.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Count(pub i32);

/// A 64-bit count of changes, used in GAP
/// (DDSI-RTPS 2.5, 8.3.5.11 and 9.3.2.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChangeCount(pub i64);

impl ChangeCount {
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        SequenceNumber::read(r).map(|s| ChangeCount(s.0))
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        SequenceNumber(self.0).write(w);
    }
}

/// Four octets that stand for a group of entities
/// (DDSI-RTPS 2.5, 9.3.2.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GroupDigest(pub [u8; 4]);

/// A transport address (DDSI-RTPS 2.5, 9.3.2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Locator {
    /// The transport; see the `KIND_*` constants.
    pub kind: i32,
    /// The port.
    pub port: u32,
    /// The address. IPv4 addresses use the last four octets.
    pub address: [u8; 16],
}

impl Locator {
    /// `LOCATOR_KIND_INVALID`.
    pub const KIND_INVALID: i32 = -1;
    /// `LOCATOR_KIND_RESERVED`.
    pub const KIND_RESERVED: i32 = 0;
    /// `LOCATOR_KIND_UDPv4`.
    pub const KIND_UDPV4: i32 = 1;
    /// `LOCATOR_KIND_UDPv6`.
    pub const KIND_UDPV6: i32 = 2;
    /// `LOCATOR_PORT_INVALID`.
    pub const PORT_INVALID: u32 = 0;
    /// `LOCATOR_ADDRESS_INVALID`.
    pub const ADDRESS_INVALID: [u8; 16] = [0; 16];
    /// `LOCATOR_INVALID`.
    pub const INVALID: Locator = Locator {
        kind: Locator::KIND_INVALID,
        port: Locator::PORT_INVALID,
        address: Locator::ADDRESS_INVALID,
    };

    /// The size of a locator on the wire.
    pub(crate) const WIRE_SIZE: usize = 24;

    /// A UDPv4 locator.
    pub fn udpv4(addr: SocketAddrV4) -> Self {
        let mut address = [0u8; 16];
        address[12..].copy_from_slice(&addr.ip().octets());
        Locator {
            kind: Locator::KIND_UDPV4,
            port: addr.port() as u32,
            address,
        }
    }

    /// A UDPv6 locator.
    pub fn udpv6(addr: SocketAddrV6) -> Self {
        Locator {
            kind: Locator::KIND_UDPV6,
            port: addr.port() as u32,
            address: addr.ip().octets(),
        }
    }

    /// The UDP socket address, if this is a UDPv4 or UDPv6 locator whose
    /// port fits in 16 bits.
    pub fn to_socket_addr(&self) -> Option<SocketAddr> {
        let port = u16::try_from(self.port).ok()?;
        match self.kind {
            Locator::KIND_UDPV4 => {
                let a = &self.address;
                let ip = Ipv4Addr::new(a[12], a[13], a[14], a[15]);
                Some(SocketAddr::V4(SocketAddrV4::new(ip, port)))
            }
            Locator::KIND_UDPV6 => Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(self.address),
                port,
                0,
                0,
            ))),
            _ => None,
        }
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Locator {
            kind: r.i32()?,
            port: r.u32()?,
            address: r.array()?,
        })
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.i32(self.kind);
        w.u32(self.port);
        w.bytes(&self.address);
    }
}

/// The compact UDPv4 locator of INFO_REPLY_IP4
/// (DDSI-RTPS 2.5, 9.4.3.1).
///
/// The specification gives both fields as `unsigned long` and does not say
/// how `address` maps to dotted notation. [`LocatorUdpV4::ip`] reads it
/// with `a` as the most significant octet; this needs to be checked
/// against other implementations (rustorb-dds#8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LocatorUdpV4 {
    /// The IPv4 address as a 32-bit number.
    pub address: u32,
    /// The port.
    pub port: u32,
}

impl LocatorUdpV4 {
    /// The address as an [`Ipv4Addr`], `a` being the most significant
    /// octet of `address`.
    pub fn ip(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.address)
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(LocatorUdpV4 {
            address: r.u32()?,
            port: r.u32()?,
        })
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>) {
        w.u32(self.address);
        w.u32(self.port);
    }
}

/// A length of time in NTP format: `seconds + fraction / 2^32`
/// (DDSI-RTPS 2.5, 9.3.2.1 and 9.3.2.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Duration {
    /// Whole seconds.
    pub seconds: i32,
    /// Fraction of a second in units of 2^-32 s.
    pub fraction: u32,
}

impl Duration {
    /// `DURATION_ZERO`.
    pub const ZERO: Duration = Duration {
        seconds: 0,
        fraction: 0,
    };
    /// `DURATION_INFINITE`.
    pub const INFINITE: Duration = Duration {
        seconds: 0x7fff_ffff,
        fraction: 0xffff_ffff,
    };

    /// Whole seconds.
    pub const fn from_secs(seconds: i32) -> Duration {
        Duration {
            seconds,
            fraction: 0,
        }
    }

    /// Converts to a [`std::time::Duration`]. Negative values give zero;
    /// [`Duration::INFINITE`] gives `None`.
    pub fn to_std(self) -> Option<std::time::Duration> {
        if self == Duration::INFINITE {
            return None;
        }
        if self.seconds < 0 {
            return Some(std::time::Duration::ZERO);
        }
        let nanos = (u64::from(self.fraction) * 1_000_000_000) >> 32;
        Some(std::time::Duration::new(self.seconds as u64, nanos as u32))
    }

    /// Converts from a [`std::time::Duration`], saturating at the largest
    /// finite value.
    pub fn from_std(d: std::time::Duration) -> Duration {
        let Ok(seconds) = i32::try_from(d.as_secs()) else {
            return Duration {
                seconds: i32::MAX,
                fraction: 0xffff_fffe,
            };
        };
        Duration {
            seconds,
            fraction: ((u64::from(d.subsec_nanos()) << 32) / 1_000_000_000) as u32,
        }
    }
}

/// The built-in endpoints a participant has (DDSI-RTPS 2.5, 9.3.2.12).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuiltinEndpointSet(pub u32);

impl BuiltinEndpointSet {
    /// `DISC_BUILTIN_ENDPOINT_PARTICIPANT_ANNOUNCER`.
    pub const PARTICIPANT_ANNOUNCER: u32 = 1 << 0;
    /// `DISC_BUILTIN_ENDPOINT_PARTICIPANT_DETECTOR`.
    pub const PARTICIPANT_DETECTOR: u32 = 1 << 1;
    /// `DISC_BUILTIN_ENDPOINT_PUBLICATIONS_ANNOUNCER`.
    pub const PUBLICATIONS_ANNOUNCER: u32 = 1 << 2;
    /// `DISC_BUILTIN_ENDPOINT_PUBLICATIONS_DETECTOR`.
    pub const PUBLICATIONS_DETECTOR: u32 = 1 << 3;
    /// `DISC_BUILTIN_ENDPOINT_SUBSCRIPTIONS_ANNOUNCER`.
    pub const SUBSCRIPTIONS_ANNOUNCER: u32 = 1 << 4;
    /// `DISC_BUILTIN_ENDPOINT_SUBSCRIPTIONS_DETECTOR`.
    pub const SUBSCRIPTIONS_DETECTOR: u32 = 1 << 5;
    /// `BUILTIN_ENDPOINT_PARTICIPANT_MESSAGE_DATA_WRITER`.
    pub const PARTICIPANT_MESSAGE_DATA_WRITER: u32 = 1 << 10;
    /// `BUILTIN_ENDPOINT_PARTICIPANT_MESSAGE_DATA_READER`.
    pub const PARTICIPANT_MESSAGE_DATA_READER: u32 = 1 << 11;
    /// TypeLookupServiceRequestDataWriter (DDS-XTypes 1.3 Table 62).
    pub const TYPE_LOOKUP_REQUEST_WRITER: u32 = 1 << 12;
    /// TypeLookupServiceRequestDataReader.
    pub const TYPE_LOOKUP_REQUEST_READER: u32 = 1 << 13;
    /// TypeLookupServiceReplyDataWriter.
    pub const TYPE_LOOKUP_REPLY_WRITER: u32 = 1 << 14;
    /// TypeLookupServiceReplyDataReader.
    pub const TYPE_LOOKUP_REPLY_READER: u32 = 1 << 15;
    /// `DISC_BUILTIN_ENDPOINT_TOPICS_ANNOUNCER`.
    pub const TOPICS_ANNOUNCER: u32 = 1 << 28;
    /// `DISC_BUILTIN_ENDPOINT_TOPICS_DETECTOR`.
    pub const TOPICS_DETECTOR: u32 = 1 << 29;

    /// Whether all the given bits are set.
    pub const fn contains(self, bits: u32) -> bool {
        self.0 & bits == bits
    }
}

#[cfg(test)]
mod sample_identity_tests {
    use super::*;

    #[test]
    fn sample_identity_round_trip() {
        let id = SampleIdentity {
            writer_guid: Guid::new(GuidPrefix([1; 12]), EntityId::PARTICIPANT),
            sequence_number: SequenceNumber((2 << 32) | 3),
        };
        let le = id.to_bytes(Endianness::Little);
        assert_eq!(&le[16..], &[2, 0, 0, 0, 3, 0, 0, 0]);
        assert_eq!(
            SampleIdentity::from_bytes(&le, Endianness::Little).unwrap(),
            id
        );
        let be = id.to_bytes(Endianness::Big);
        assert_eq!(&be[16..], &[0, 0, 0, 2, 0, 0, 0, 3]);
        assert_eq!(
            SampleIdentity::from_bytes(&be, Endianness::Big).unwrap(),
            id
        );
        assert!(SampleIdentity::from_bytes(&be[..20], Endianness::Big).is_err());
    }
}
