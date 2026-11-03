//! The DDSI-RTPS 2.5 wire protocol for rustorb-dds.
//!
//! What is here so far: reading and writing RTPS messages and
//! submessages (DDSI-RTPS 2.5 clauses 8.3 and 9.4, rustorb-dds#3), and the
//! UDP transport (9.6.2, rustorb-dds#4).
//!
//! - [`Message`], [`Header`] and the [`Submessage`] kinds. All submessage
//!   kinds of version 2.5 are handled; unknown and vendor-specific ones
//!   are kept as [`UnknownSubmessage`] and skipped by receivers.
//! - The basic types: [`Guid`], [`EntityId`], [`SequenceNumber`],
//!   [`SequenceNumberSet`], [`Time`], [`Locator`] and so on.
//! - The UDPv4 transport and the default port numbers, in [`udp`]
//!   (rustorb-dds#4).
//! - [`ParameterList`] framing. Parameter values and serialized payloads
//!   stay as raw octets; PL_CDR and the payload encodings are read by
//!   higher layers.
//!
//! ```
//! use rustorb_rtps::*;
//!
//! let mut msg = Message::new(Header::new(VendorId([0x01, 0x02]), GuidPrefix([7; 12])));
//! msg.submessages.push(Submessage::InfoTimestamp(InfoTimestamp {
//!     timestamp: Some(Time { seconds: 1, fraction: 0 }),
//! }));
//! msg.submessages.push(Submessage::Data(Data {
//!     reader_id: EntityId::UNKNOWN,
//!     writer_id: EntityId::user([0, 0, 1], entity_kind::USER_WRITER_NO_KEY),
//!     writer_sn: SequenceNumber(1),
//!     inline_qos: None,
//!     serialized_payload: Some(SerializedPayload {
//!         kind: PayloadKind::Data,
//!         bytes: vec![0x00, 0x01, 0x00, 0x00, 1, 0, 0, 0],
//!     }),
//!     non_standard_payload: false,
//! }));
//! let bytes = msg.encode(Endianness::Little).unwrap();
//! assert_eq!(Message::decode(&bytes).unwrap(), msg);
//! ```
//!
//! Still to come (rustorb/rustorb#111, stages D1 and D2): SPDP/SEDP
//! discovery, and best-effort then reliable writers
//! and readers.

mod error;
mod message;
mod parameter;
mod submessage;
mod types;
pub mod udp;
mod wire;

pub use error::{DecodeError, EncodeError};
pub use message::{Header, Message, PROTOCOL_RTPS, Submessages};
pub use parameter::{Parameter, ParameterId, ParameterList};
pub use submessage::{
    AckNack, Checksum, Data, DataFrag, Gap, HeaderExtension, Heartbeat, HeartbeatFrag,
    HeartbeatGroupInfo, InfoDestination, InfoReply, InfoReplyIp4, InfoSource, InfoTimestamp,
    NackFrag, Pad, PayloadKind, SerializedPayload, Submessage, UnknownSubmessage, submessage_id,
};
pub use types::{
    ChangeCount, Count, EntityId, FragmentNumber, FragmentNumberSet, GroupDigest, Guid, GuidPrefix,
    Locator, LocatorUdpV4, ProtocolVersion, SequenceNumber, SequenceNumberSet, Time, VendorId,
    entity_kind,
};
pub use wire::Endianness;
