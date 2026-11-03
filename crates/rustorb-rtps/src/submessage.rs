//! RTPS submessages (DDSI-RTPS 2.5, 8.3.7, 8.3.8 and 9.4.5).
//!
//! Each submessage type here holds the decoded elements. The
//! EndiannessFlag is not stored: the byte order is chosen when a message
//! is encoded. Flags marked `X` (unused) in the specification are ignored
//! when reading and written as zero (9.4.5.1.2).

use crate::error::{DecodeError, EncodeError};
use crate::parameter::ParameterList;
use crate::types::{
    ChangeCount, Count, EntityId, FragmentNumber, FragmentNumberSet, GroupDigest, GuidPrefix,
    Locator, LocatorUdpV4, ProtocolVersion, SequenceNumber, SequenceNumberSet, Time, VendorId,
};
use crate::wire::{Reader, Writer};

/// Submessage ids (DDSI-RTPS 2.5, 9.4.5.1.1).
///
/// Ids `0x80..=0xff` are vendor-specific.
pub mod submessage_id {
    /// HeaderExtension.
    pub const RTPS_HE: u8 = 0x00;
    /// Pad.
    pub const PAD: u8 = 0x01;
    /// AckNack.
    pub const ACKNACK: u8 = 0x06;
    /// Heartbeat.
    pub const HEARTBEAT: u8 = 0x07;
    /// Gap.
    pub const GAP: u8 = 0x08;
    /// InfoTimestamp.
    pub const INFO_TS: u8 = 0x09;
    /// InfoSource.
    pub const INFO_SRC: u8 = 0x0c;
    /// InfoReplyIp4.
    pub const INFO_REPLY_IP4: u8 = 0x0d;
    /// InfoDestination.
    pub const INFO_DST: u8 = 0x0e;
    /// InfoReply.
    pub const INFO_REPLY: u8 = 0x0f;
    /// NackFrag.
    pub const NACK_FRAG: u8 = 0x12;
    /// HeartbeatFrag.
    pub const HEARTBEAT_FRAG: u8 = 0x13;
    /// Data.
    pub const DATA: u8 = 0x15;
    /// DataFrag.
    pub const DATA_FRAG: u8 = 0x16;
}

use submessage_id as id;

/// One submessage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Submessage {
    /// RTPS_HE (8.3.7, 9.4.5.2).
    HeaderExtension(HeaderExtension),
    /// PAD (8.3.8.13, 9.4.5.13).
    Pad(Pad),
    /// ACKNACK (8.3.8.1, 9.4.5.3).
    AckNack(AckNack),
    /// HEARTBEAT (8.3.8.6, 9.4.5.7).
    Heartbeat(Heartbeat),
    /// GAP (8.3.8.4, 9.4.5.6).
    Gap(Gap),
    /// INFO_TS (8.3.8.11, 9.4.5.12).
    InfoTimestamp(InfoTimestamp),
    /// INFO_SRC (8.3.8.10, 9.4.5.11).
    InfoSource(InfoSource),
    /// INFO_REPLY_IP4 (9.4.5.15).
    InfoReplyIp4(InfoReplyIp4),
    /// INFO_DST (8.3.8.8, 9.4.5.9).
    InfoDestination(InfoDestination),
    /// INFO_REPLY (8.3.8.9, 9.4.5.10).
    InfoReply(InfoReply),
    /// NACK_FRAG (8.3.8.12, 9.4.5.14).
    NackFrag(NackFrag),
    /// HEARTBEAT_FRAG (8.3.8.7, 9.4.5.8).
    HeartbeatFrag(HeartbeatFrag),
    /// DATA (8.3.8.2, 9.4.5.4).
    Data(Data),
    /// DATA_FRAG (8.3.8.3, 9.4.5.5).
    DataFrag(DataFrag),
    /// A submessage this crate does not know: a vendor-specific one, or one
    /// from a later minor version. Receivers skip these (8.3.4.1 rule 3).
    Unknown(UnknownSubmessage),
}

/// The checksum of a HeaderExtension (9.4.2.15, Table 9.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Checksum {
    /// CRC-32C, big-endian (flags C1,C2 = 0,1).
    Crc32([u8; 4]),
    /// CRC-64/XZ, big-endian (flags C1,C2 = 1,0).
    Crc64([u8; 8]),
    /// MD5 (flags C1,C2 = 1,1).
    Md5([u8; 16]),
}

/// RTPS_HE: optional information that belongs to the message header
/// (8.3.7). It must be the first submessage.
///
/// The checksum is carried as received; computing and checking it is not
/// done here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeaderExtension {
    /// `messageLength` (flag L).
    pub message_length: Option<u32>,
    /// `rtpsSendTimestamp` (flag T).
    pub send_timestamp: Option<Time>,
    /// `uExtension4` (flag U).
    pub u_extension4: Option<[u8; 4]>,
    /// `wExtension8` (flag W).
    pub w_extension8: Option<[u8; 8]>,
    /// `messageChecksum` (flags C1, C2).
    pub checksum: Option<Checksum>,
    /// `parameters` (flag P).
    pub parameters: Option<ParameterList>,
}

/// PAD: padding with no meaning (8.3.8.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    /// The number of padding octets after the submessage header.
    pub len: usize,
}

/// ACKNACK: a reader tells a writer what it has and what it misses
/// (8.3.8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckNack {
    /// FinalFlag: the reader does not need a HEARTBEAT in reply.
    pub final_flag: bool,
    /// The reader.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// Everything before `base` is acknowledged; the members are missing.
    pub reader_sn_state: SequenceNumberSet,
    /// Incremented with each new ACKNACK.
    pub count: Count,
}

/// The group information of a HEARTBEAT (flag G, 8.3.8.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeartbeatGroupInfo {
    /// `currentGSN`.
    pub current_gsn: SequenceNumber,
    /// `firstGSN`.
    pub first_gsn: SequenceNumber,
    /// `lastGSN`.
    pub last_gsn: SequenceNumber,
    /// `writerSet` and `secureWriterSet`.
    ///
    /// Table 8.44 lists these two elements but the wire drawing in 9.4.5.7
    /// does not. They are read when eight more octets are present and
    /// written when set. To be checked against other implementations
    /// (rustorb-dds#8).
    pub writer_sets: Option<(GroupDigest, GroupDigest)>,
}

/// HEARTBEAT: a writer tells readers which sequence numbers it has
/// (8.3.8.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// FinalFlag: the reader does not have to reply.
    pub final_flag: bool,
    /// LivelinessFlag: the DataWriter asserted its liveliness manually.
    pub liveliness_flag: bool,
    /// The reader, or [`EntityId::UNKNOWN`] for all readers.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// The first sequence number still available.
    pub first_sn: SequenceNumber,
    /// The last sequence number ever written.
    pub last_sn: SequenceNumber,
    /// Incremented with each new HEARTBEAT.
    pub count: Count,
    /// Group information (flag G).
    pub group_info: Option<HeartbeatGroupInfo>,
}

/// GAP: a writer tells a reader that some changes will not be sent
/// (8.3.8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    /// The reader, or [`EntityId::UNKNOWN`] for all matched readers.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// First sequence number of the irrelevant range.
    pub gap_start: SequenceNumber,
    /// The range ends at `gap_list.base() - 1`; the members are irrelevant
    /// too.
    pub gap_list: SequenceNumberSet,
    /// `gapStartGSN` and `gapEndGSN` (flag G).
    pub group_info: Option<(SequenceNumber, SequenceNumber)>,
    /// `filteredCount` (flag F).
    pub filtered_count: Option<ChangeCount>,
}

/// INFO_TS: the source timestamp of the submessages that follow
/// (8.3.8.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoTimestamp {
    /// The timestamp, or `None` when the InvalidateFlag is set.
    pub timestamp: Option<Time>,
}

/// INFO_SRC: the logical source of the submessages that follow
/// (8.3.8.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoSource {
    /// The protocol version of the source.
    pub protocol_version: ProtocolVersion,
    /// The vendor of the source.
    pub vendor_id: VendorId,
    /// The participant of the source.
    pub guid_prefix: GuidPrefix,
}

/// INFO_DST: the participant the submessages that follow are for
/// (8.3.8.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoDestination {
    /// The destination participant, or [`GuidPrefix::UNKNOWN`] for the
    /// receiving one.
    pub guid_prefix: GuidPrefix,
}

/// INFO_REPLY: where to send replies to the submessages that follow
/// (8.3.8.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoReply {
    /// Unicast reply locators.
    pub unicast_locators: Vec<Locator>,
    /// Multicast reply locators (flag M).
    pub multicast_locators: Option<Vec<Locator>>,
}

/// INFO_REPLY_IP4: the compact UDPv4 form of INFO_REPLY (9.4.5.15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoReplyIp4 {
    /// Unicast reply locator.
    pub unicast_locator: LocatorUdpV4,
    /// Multicast reply locator (flag M).
    pub multicast_locator: Option<LocatorUdpV4>,
}

/// NACK_FRAG: a reader asks for missing fragments (8.3.8.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NackFrag {
    /// The reader.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// The change whose fragments are missing.
    pub writer_sn: SequenceNumber,
    /// The missing fragments.
    pub fragment_number_state: FragmentNumberSet,
    /// Incremented with each new NACK_FRAG.
    pub count: Count,
}

/// HEARTBEAT_FRAG: a writer tells readers which fragments it has
/// (8.3.8.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeartbeatFrag {
    /// The reader, or [`EntityId::UNKNOWN`] for all readers.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// The change.
    pub writer_sn: SequenceNumber,
    /// All fragments up to this one are available.
    pub last_fragment_num: FragmentNumber,
    /// Incremented with each new HEARTBEAT_FRAG.
    pub count: Count,
}

/// What a serialized payload holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PayloadKind {
    /// The value of the data object (DataFlag).
    Data,
    /// The key of the data object (KeyFlag).
    Key,
}

/// The serialized payload of a DATA submessage, kept as octets
/// (8.3.5.17, 9.4.2.12, clause 10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedPayload {
    /// Value or key.
    pub kind: PayloadKind,
    /// The octets, starting with the 4-octet SerializedPayloadHeader.
    ///
    /// When read, this runs to the end of the submessage and so may
    /// include the padding that aligns the next submessage. When written,
    /// it is padded with zeros to a multiple of four.
    pub bytes: Vec<u8>,
}

/// DATA: a change to a data object, sent by a writer (8.3.8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    /// The reader, or [`EntityId::UNKNOWN`] for all matched readers.
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// The sequence number of the change.
    pub writer_sn: SequenceNumber,
    /// In-line QoS (flag Q).
    pub inline_qos: Option<ParameterList>,
    /// The payload (flag D or K).
    pub serialized_payload: Option<SerializedPayload>,
    /// NonStandardPayloadFlag: the payload is not formatted as in clause
    /// 10 (for example, it was transformed by DDS-Security).
    pub non_standard_payload: bool,
}

/// DATA_FRAG: some fragments of a change (8.3.8.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataFrag {
    /// KeyFlag: the fragments are of the key, not the value.
    pub key_flag: bool,
    /// NonStandardPayloadFlag.
    pub non_standard_payload: bool,
    /// The reader, or [`EntityId::UNKNOWN`].
    pub reader_id: EntityId,
    /// The writer.
    pub writer_id: EntityId,
    /// The sequence number of the change.
    pub writer_sn: SequenceNumber,
    /// The first fragment in this submessage (from 1).
    pub fragment_starting_num: FragmentNumber,
    /// How many consecutive fragments this submessage holds.
    pub fragments_in_submessage: u16,
    /// The size of each fragment.
    pub fragment_size: u16,
    /// The size of the whole serialized data (`sampleSize`, called
    /// `dataSize` in 8.3.8.3).
    pub sample_size: u32,
    /// In-line QoS (flag Q).
    pub inline_qos: Option<ParameterList>,
    /// The fragment octets.
    pub serialized_payload: Vec<u8>,
}

/// A submessage with an id this crate does not handle, kept as octets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownSubmessage {
    /// The submessage id.
    pub id: u8,
    /// The flags, including the EndiannessFlag, as received.
    pub flags: u8,
    /// The contents after the submessage header.
    pub body: Vec<u8>,
}

const TOO_SMALL: &str = "submessageLength too small";

fn invalid(id: u8, reason: &'static str) -> DecodeError {
    DecodeError::InvalidSubmessage { id, reason }
}

fn flag(flags: u8, bit: u8) -> bool {
    flags & bit != 0
}

impl Submessage {
    /// The submessage id.
    pub fn id(&self) -> u8 {
        match self {
            Submessage::HeaderExtension(_) => id::RTPS_HE,
            Submessage::Pad(_) => id::PAD,
            Submessage::AckNack(_) => id::ACKNACK,
            Submessage::Heartbeat(_) => id::HEARTBEAT,
            Submessage::Gap(_) => id::GAP,
            Submessage::InfoTimestamp(_) => id::INFO_TS,
            Submessage::InfoSource(_) => id::INFO_SRC,
            Submessage::InfoReplyIp4(_) => id::INFO_REPLY_IP4,
            Submessage::InfoDestination(_) => id::INFO_DST,
            Submessage::InfoReply(_) => id::INFO_REPLY,
            Submessage::NackFrag(_) => id::NACK_FRAG,
            Submessage::HeartbeatFrag(_) => id::HEARTBEAT_FRAG,
            Submessage::Data(_) => id::DATA,
            Submessage::DataFrag(_) => id::DATA_FRAG,
            Submessage::Unknown(u) => u.id,
        }
    }

    /// Reads the contents of a submessage whose header gave `id` and
    /// `flags`. `body` is everything after the header up to the next one.
    pub(crate) fn decode_body(id: u8, flags: u8, body: &[u8]) -> Result<Submessage, DecodeError> {
        let mut r = Reader::new(body, crate::wire::Endianness::from_flags(flags));
        let result = Self::read_body(id, flags, &mut r);
        result.map_err(|e| match e {
            DecodeError::UnexpectedEnd => invalid(id, TOO_SMALL),
            DecodeError::InvalidParameterList(_) => invalid(id, "invalid parameter list"),
            e => e,
        })
    }

    fn read_body(id: u8, flags: u8, r: &mut Reader<'_>) -> Result<Submessage, DecodeError> {
        Ok(match id {
            id::RTPS_HE => Submessage::HeaderExtension(HeaderExtension::read(flags, r)?),
            id::PAD => Submessage::Pad(Pad {
                len: r.rest().len(),
            }),
            id::ACKNACK => Submessage::AckNack(AckNack::read(flags, r)?),
            id::HEARTBEAT => Submessage::Heartbeat(Heartbeat::read(flags, r)?),
            id::GAP => Submessage::Gap(Gap::read(flags, r)?),
            id::INFO_TS => Submessage::InfoTimestamp(InfoTimestamp {
                timestamp: if flag(flags, 0x02) {
                    None
                } else {
                    Some(Time::read(r)?)
                },
            }),
            id::INFO_SRC => {
                r.skip(4)?; // unused
                Submessage::InfoSource(InfoSource {
                    protocol_version: ProtocolVersion::read(r)?,
                    vendor_id: VendorId::read(r)?,
                    guid_prefix: GuidPrefix::read(r)?,
                })
            }
            id::INFO_REPLY_IP4 => Submessage::InfoReplyIp4(InfoReplyIp4 {
                unicast_locator: LocatorUdpV4::read(r)?,
                multicast_locator: if flag(flags, 0x02) {
                    Some(LocatorUdpV4::read(r)?)
                } else {
                    None
                },
            }),
            id::INFO_DST => Submessage::InfoDestination(InfoDestination {
                guid_prefix: GuidPrefix::read(r)?,
            }),
            id::INFO_REPLY => Submessage::InfoReply(InfoReply {
                unicast_locators: read_locator_list(r)?,
                multicast_locators: if flag(flags, 0x02) {
                    Some(read_locator_list(r)?)
                } else {
                    None
                },
            }),
            id::NACK_FRAG => Submessage::NackFrag(NackFrag::read(r)?),
            id::HEARTBEAT_FRAG => Submessage::HeartbeatFrag(HeartbeatFrag::read(r)?),
            id::DATA => Submessage::Data(Data::read(flags, r)?),
            id::DATA_FRAG => Submessage::DataFrag(DataFrag::read(flags, r)?),
            _ => Submessage::Unknown(UnknownSubmessage {
                id,
                flags,
                body: r.rest().to_vec(),
            }),
        })
    }

    /// Writes the contents (not the header) and returns the flags to put
    /// in the header, including the EndiannessFlag.
    pub(crate) fn write_body(&self, w: &mut Writer<'_>) -> Result<u8, EncodeError> {
        let e = w.endianness().flag();
        Ok(match self {
            Submessage::HeaderExtension(he) => e | he.write(w)?,
            Submessage::Pad(p) => {
                w.zeros(p.len);
                e
            }
            Submessage::AckNack(a) => e | a.write(w),
            Submessage::Heartbeat(h) => e | h.write(w),
            Submessage::Gap(g) => e | g.write(w),
            Submessage::InfoTimestamp(ts) => match &ts.timestamp {
                Some(t) => {
                    t.write(w);
                    e
                }
                None => e | 0x02,
            },
            Submessage::InfoSource(s) => {
                w.zeros(4);
                s.protocol_version.write(w);
                s.vendor_id.write(w);
                s.guid_prefix.write(w);
                e
            }
            Submessage::InfoReplyIp4(ir) => {
                ir.unicast_locator.write(w);
                match &ir.multicast_locator {
                    Some(m) => {
                        m.write(w);
                        e | 0x02
                    }
                    None => e,
                }
            }
            Submessage::InfoDestination(d) => {
                d.guid_prefix.write(w);
                e
            }
            Submessage::InfoReply(ir) => {
                write_locator_list(w, &ir.unicast_locators)?;
                match &ir.multicast_locators {
                    Some(m) => {
                        write_locator_list(w, m)?;
                        e | 0x02
                    }
                    None => e,
                }
            }
            Submessage::NackFrag(n) => {
                n.reader_id.write(w);
                n.writer_id.write(w);
                n.writer_sn.write(w);
                n.fragment_number_state.write(w);
                w.i32(n.count.0);
                e
            }
            Submessage::HeartbeatFrag(h) => {
                h.reader_id.write(w);
                h.writer_id.write(w);
                h.writer_sn.write(w);
                w.u32(h.last_fragment_num.0);
                w.i32(h.count.0);
                e
            }
            Submessage::Data(d) => e | d.write(w)?,
            Submessage::DataFrag(d) => e | d.write(w)?,
            Submessage::Unknown(u) => {
                w.bytes(&u.body);
                u.flags
            }
        })
    }
}

fn read_locator_list(r: &mut Reader<'_>) -> Result<Vec<Locator>, DecodeError> {
    let n = r.u32()? as usize;
    // Check the size first so that a bad count cannot make us allocate.
    if n > r.remaining() / Locator::WIRE_SIZE {
        return Err(DecodeError::UnexpectedEnd);
    }
    (0..n).map(|_| Locator::read(r)).collect()
}

fn write_locator_list(w: &mut Writer<'_>, list: &[Locator]) -> Result<(), EncodeError> {
    w.u32(u32::try_from(list.len()).map_err(|_| EncodeError::TooManyLocators)?);
    for l in list {
        l.write(w);
    }
    Ok(())
}

impl HeaderExtension {
    const L: u8 = 0x02;
    const T: u8 = 0x04;
    const U: u8 = 0x08;
    const W: u8 = 0x10;
    const C2: u8 = 0x20;
    const C1: u8 = 0x40;
    const P: u8 = 0x80;

    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let body_len = r.remaining();
        let message_length = if flag(flags, Self::L) {
            Some(r.u32()?)
        } else {
            None
        };
        let send_timestamp = if flag(flags, Self::T) {
            Some(Time::read(r)?)
        } else {
            None
        };
        let u_extension4 = if flag(flags, Self::U) {
            Some(r.array()?)
        } else {
            None
        };
        let w_extension8 = if flag(flags, Self::W) {
            Some(r.array()?)
        } else {
            None
        };
        let checksum = match (flag(flags, Self::C1), flag(flags, Self::C2)) {
            (false, false) => None,
            (false, true) => Some(Checksum::Crc32(r.array()?)),
            (true, false) => Some(Checksum::Crc64(r.array()?)),
            (true, true) => Some(Checksum::Md5(r.array()?)),
        };
        let parameters = if flag(flags, Self::P) {
            Some(ParameterList::read(r, false)?)
        } else {
            None
        };
        // 8.3.7.3: messageLength must fit the Header (20 octets), this
        // submessage's header (4) and its contents.
        if let Some(len) = message_length {
            if (len as usize) < 20 + 4 + body_len {
                return Err(invalid(id::RTPS_HE, "messageLength too small"));
            }
        }
        Ok(HeaderExtension {
            message_length,
            send_timestamp,
            u_extension4,
            w_extension8,
            checksum,
            parameters,
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<u8, EncodeError> {
        let mut flags = 0;
        if let Some(len) = self.message_length {
            flags |= Self::L;
            w.u32(len);
        }
        if let Some(t) = &self.send_timestamp {
            flags |= Self::T;
            t.write(w);
        }
        if let Some(u) = &self.u_extension4 {
            flags |= Self::U;
            w.bytes(u);
        }
        if let Some(x) = &self.w_extension8 {
            flags |= Self::W;
            w.bytes(x);
        }
        match &self.checksum {
            None => {}
            Some(Checksum::Crc32(c)) => {
                flags |= Self::C2;
                w.bytes(c);
            }
            Some(Checksum::Crc64(c)) => {
                flags |= Self::C1;
                w.bytes(c);
            }
            Some(Checksum::Md5(c)) => {
                flags |= Self::C1 | Self::C2;
                w.bytes(c);
            }
        }
        if let Some(p) = &self.parameters {
            flags |= Self::P;
            p.write(w, true)?;
        }
        Ok(flags)
    }
}

impl AckNack {
    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(AckNack {
            final_flag: flag(flags, 0x02),
            reader_id: EntityId::read(r)?,
            writer_id: EntityId::read(r)?,
            reader_sn_state: SequenceNumberSet::read(r, id::ACKNACK)?,
            count: Count(r.i32()?),
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> u8 {
        self.reader_id.write(w);
        self.writer_id.write(w);
        self.reader_sn_state.write(w);
        w.i32(self.count.0);
        if self.final_flag { 0x02 } else { 0 }
    }
}

impl Heartbeat {
    const F: u8 = 0x02;
    const L: u8 = 0x04;
    const G: u8 = 0x08;

    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let err = |reason| invalid(id::HEARTBEAT, reason);
        let reader_id = EntityId::read(r)?;
        let writer_id = EntityId::read(r)?;
        let first_sn = SequenceNumber::read(r)?;
        let last_sn = SequenceNumber::read(r)?;
        let count = Count(r.i32()?);
        // Validity (8.3.8.6.3).
        if first_sn.0 <= 0 {
            return Err(err("firstSN is not positive"));
        }
        if last_sn.0 < 0 {
            return Err(err("lastSN is negative"));
        }
        if last_sn.0 < first_sn.0 - 1 {
            return Err(err("lastSN < firstSN - 1"));
        }
        let group_info = if flag(flags, Self::G) {
            let current_gsn = SequenceNumber::read(r)?;
            let first_gsn = SequenceNumber::read(r)?;
            let last_gsn = SequenceNumber::read(r)?;
            let writer_sets = if r.remaining() >= 8 {
                Some((GroupDigest(r.array()?), GroupDigest(r.array()?)))
            } else {
                None
            };
            if current_gsn.0 <= 0 || first_gsn.0 <= 0 {
                return Err(err("group sequence number is not positive"));
            }
            if last_gsn.0 < 0 || last_gsn.0 < first_gsn.0 - 1 {
                return Err(err("lastGSN < firstGSN - 1"));
            }
            if current_gsn.0 < first_gsn.0 {
                return Err(err("currentGSN < firstGSN"));
            }
            // 8.3.8.6.3 also calls "currentGSN > lastGSN" invalid, but
            // currentGSN is the highest GSN of the whole group, so it is
            // normally >= lastGSN. That rule is not applied until other
            // implementations show which reading is meant (rustorb-dds#8).
            Some(HeartbeatGroupInfo {
                current_gsn,
                first_gsn,
                last_gsn,
                writer_sets,
            })
        } else {
            None
        };
        Ok(Heartbeat {
            final_flag: flag(flags, Self::F),
            liveliness_flag: flag(flags, Self::L),
            reader_id,
            writer_id,
            first_sn,
            last_sn,
            count,
            group_info,
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> u8 {
        let mut flags = 0;
        if self.final_flag {
            flags |= Self::F;
        }
        if self.liveliness_flag {
            flags |= Self::L;
        }
        self.reader_id.write(w);
        self.writer_id.write(w);
        self.first_sn.write(w);
        self.last_sn.write(w);
        w.i32(self.count.0);
        if let Some(g) = &self.group_info {
            flags |= Self::G;
            g.current_gsn.write(w);
            g.first_gsn.write(w);
            g.last_gsn.write(w);
            if let Some((a, b)) = &g.writer_sets {
                w.bytes(&a.0);
                w.bytes(&b.0);
            }
        }
        flags
    }
}

impl Gap {
    const G: u8 = 0x02;
    const F: u8 = 0x04;

    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let err = |reason| invalid(id::GAP, reason);
        let reader_id = EntityId::read(r)?;
        let writer_id = EntityId::read(r)?;
        let gap_start = SequenceNumber::read(r)?;
        let gap_list = SequenceNumberSet::read(r, id::GAP)?;
        if gap_start.0 <= 0 {
            return Err(err("gapStart is not positive"));
        }
        let group_info = if flag(flags, Self::G) {
            let start = SequenceNumber::read(r)?;
            let end = SequenceNumber::read(r)?;
            if start.0 <= 0 || end.0 <= 0 {
                return Err(err("group sequence number is not positive"));
            }
            if end.0 < start.0 - 1 {
                return Err(err("gapEndGSN < gapStartGSN - 1"));
            }
            Some((start, end))
        } else {
            None
        };
        let filtered_count = if flag(flags, Self::F) {
            Some(ChangeCount::read(r)?)
        } else {
            None
        };
        Ok(Gap {
            reader_id,
            writer_id,
            gap_start,
            gap_list,
            group_info,
            filtered_count,
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> u8 {
        let mut flags = 0;
        self.reader_id.write(w);
        self.writer_id.write(w);
        self.gap_start.write(w);
        self.gap_list.write(w);
        if let Some((start, end)) = &self.group_info {
            flags |= Self::G;
            start.write(w);
            end.write(w);
        }
        if let Some(c) = &self.filtered_count {
            flags |= Self::F;
            c.write(w);
        }
        flags
    }
}

impl NackFrag {
    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let reader_id = EntityId::read(r)?;
        let writer_id = EntityId::read(r)?;
        let writer_sn = SequenceNumber::read(r)?;
        let fragment_number_state = FragmentNumberSet::read(r, id::NACK_FRAG)?;
        let count = Count(r.i32()?);
        if writer_sn.0 <= 0 {
            return Err(invalid(id::NACK_FRAG, "writerSN is not positive"));
        }
        Ok(NackFrag {
            reader_id,
            writer_id,
            writer_sn,
            fragment_number_state,
            count,
        })
    }
}

impl HeartbeatFrag {
    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let reader_id = EntityId::read(r)?;
        let writer_id = EntityId::read(r)?;
        let writer_sn = SequenceNumber::read(r)?;
        let last_fragment_num = FragmentNumber(r.u32()?);
        let count = Count(r.i32()?);
        if writer_sn.0 <= 0 {
            return Err(invalid(id::HEARTBEAT_FRAG, "writerSN is not positive"));
        }
        if last_fragment_num.0 == 0 {
            return Err(invalid(id::HEARTBEAT_FRAG, "lastFragmentNum is zero"));
        }
        Ok(HeartbeatFrag {
            reader_id,
            writer_id,
            writer_sn,
            last_fragment_num,
            count,
        })
    }
}

/// Reads `extraFlags` and `octetsToInlineQos`, then the fixed elements
/// with `read_fixed`, then skips whatever a later protocol version put
/// before the in-line QoS (9.4.5.4.3) and reads the in-line QoS.
fn read_data_common<'a, T>(
    sub_id: u8,
    flags: u8,
    r: &mut Reader<'a>,
    fixed_len: usize,
    read_fixed: impl FnOnce(&mut Reader<'a>) -> Result<T, DecodeError>,
) -> Result<(T, Option<ParameterList>), DecodeError> {
    let _extra_flags = r.u16()?;
    let octets_to_inline_qos = r.u16()? as usize;
    if octets_to_inline_qos < fixed_len {
        return Err(invalid(sub_id, "octetsToInlineQos too small"));
    }
    let fixed = read_fixed(r)?;
    r.skip(octets_to_inline_qos - fixed_len)?;
    let inline_qos = if flag(flags, 0x02) {
        Some(ParameterList::read(r, true).map_err(|_| invalid(sub_id, "invalid inlineQos"))?)
    } else {
        None
    };
    Ok((fixed, inline_qos))
}

impl Data {
    const Q: u8 = 0x02;
    const D: u8 = 0x04;
    const K: u8 = 0x08;
    const N: u8 = 0x10;
    /// readerId + writerId + writerSN.
    const FIXED_LEN: usize = 16;

    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let ((reader_id, writer_id, writer_sn), inline_qos) =
            read_data_common(id::DATA, flags, r, Self::FIXED_LEN, |r| {
                Ok((
                    EntityId::read(r)?,
                    EntityId::read(r)?,
                    SequenceNumber::read(r)?,
                ))
            })?;
        if writer_sn.0 <= 0 {
            return Err(invalid(id::DATA, "writerSN is not positive"));
        }
        let kind = match (flag(flags, Self::D), flag(flags, Self::K)) {
            (false, false) => None,
            (true, false) => Some(PayloadKind::Data),
            (false, true) => Some(PayloadKind::Key),
            (true, true) => return Err(invalid(id::DATA, "both DataFlag and KeyFlag set")),
        };
        let serialized_payload = kind.map(|kind| SerializedPayload {
            kind,
            bytes: r.rest().to_vec(),
        });
        Ok(Data {
            reader_id,
            writer_id,
            writer_sn,
            inline_qos,
            serialized_payload,
            non_standard_payload: flag(flags, Self::N),
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<u8, EncodeError> {
        let mut flags = 0;
        w.u16(0); // extraFlags
        w.u16(Self::FIXED_LEN as u16);
        self.reader_id.write(w);
        self.writer_id.write(w);
        self.writer_sn.write(w);
        if let Some(q) = &self.inline_qos {
            flags |= Self::Q;
            q.write(w, true)?;
        }
        if let Some(p) = &self.serialized_payload {
            flags |= match p.kind {
                PayloadKind::Data => Self::D,
                PayloadKind::Key => Self::K,
            };
            w.bytes(&p.bytes);
        }
        if self.non_standard_payload {
            flags |= Self::N;
        }
        Ok(flags)
    }
}

impl DataFrag {
    const Q: u8 = 0x02;
    const K: u8 = 0x04;
    const N: u8 = 0x08;
    /// readerId + writerId + writerSN + fragmentStartingNum +
    /// fragmentsInSubmessage + fragmentSize + sampleSize.
    const FIXED_LEN: usize = 28;

    fn read(flags: u8, r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let err = |reason| invalid(id::DATA_FRAG, reason);
        let ((reader_id, writer_id, writer_sn, start, count, size, sample_size), inline_qos) =
            read_data_common(id::DATA_FRAG, flags, r, Self::FIXED_LEN, |r| {
                Ok((
                    EntityId::read(r)?,
                    EntityId::read(r)?,
                    SequenceNumber::read(r)?,
                    FragmentNumber(r.u32()?),
                    r.u16()?,
                    r.u16()?,
                    r.u32()?,
                ))
            })?;
        // Validity (8.3.8.3.3).
        if writer_sn.0 <= 0 {
            return Err(err("writerSN is not positive"));
        }
        if size == 0 {
            return Err(err("fragmentSize is zero"));
        }
        if size as u32 > sample_size {
            return Err(err("fragmentSize exceeds sampleSize"));
        }
        let total = sample_size.div_ceil(size as u32);
        if start.0 == 0 || start.0 > total {
            return Err(err("fragmentStartingNum out of range"));
        }
        let mut payload = r.rest();
        // The payload may not be longer than fragmentsInSubmessage *
        // fragmentSize. The octets that pad the submessage to a 4-octet
        // boundary are allowed on top of that, then dropped, as are octets
        // past the end of the sample.
        let declared = count as usize * size as usize;
        if payload.len() > declared.next_multiple_of(4) {
            return Err(err("serializedPayload longer than the fragments"));
        }
        let offset = (start.0 as u64 - 1) * size as u64;
        let left_in_sample = (sample_size as u64).saturating_sub(offset) as usize;
        payload = &payload[..payload.len().min(declared).min(left_in_sample)];
        Ok(DataFrag {
            key_flag: flag(flags, Self::K),
            non_standard_payload: flag(flags, Self::N),
            reader_id,
            writer_id,
            writer_sn,
            fragment_starting_num: start,
            fragments_in_submessage: count,
            fragment_size: size,
            sample_size,
            inline_qos,
            serialized_payload: payload.to_vec(),
        })
    }

    fn write(&self, w: &mut Writer<'_>) -> Result<u8, EncodeError> {
        let mut flags = 0;
        w.u16(0); // extraFlags
        w.u16(Self::FIXED_LEN as u16);
        self.reader_id.write(w);
        self.writer_id.write(w);
        self.writer_sn.write(w);
        w.u32(self.fragment_starting_num.0);
        w.u16(self.fragments_in_submessage);
        w.u16(self.fragment_size);
        w.u32(self.sample_size);
        if let Some(q) = &self.inline_qos {
            flags |= Self::Q;
            q.write(w, true)?;
        }
        w.bytes(&self.serialized_payload);
        if self.key_flag {
            flags |= Self::K;
        }
        if self.non_standard_payload {
            flags |= Self::N;
        }
        Ok(flags)
    }
}
