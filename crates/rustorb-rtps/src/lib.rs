//! The DDSI-RTPS 2.5 wire protocol for rustorb-dds.
//!
//! What is here so far: reading and writing RTPS messages and
//! submessages (DDSI-RTPS 2.5 clauses 8.3 and 9.4, rustorb-dds#3), the
//! UDP transport (9.6.2, rustorb-dds#4), participant discovery (SPDP,
//! 8.5.3, rustorb-dds#5) in [`spdp`] with the PL_CDR helpers in [`pl`],
//! and endpoint discovery and matching (SEDP, 8.5.4, rustorb-dds#6) in
//! [`sedp`], built on the reliable writer and reader of [`reliable`], the
//! message receiver of [`receiver`] and the QoS policies of [`qos`].
//! Changes too large for a datagram are split into DATA_FRAG and put back
//! together by [`frag`] (8.4.14.1, rustorb-dds#18). The Writer Liveliness
//! Protocol (8.4.13, rustorb-dds#19) is in [`wlp`], and the endpoints of
//! the DDS-XTypes TypeLookup service (rustorb-dds#33) in [`typelookup`].
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
//! The user writers and readers built on these are in rustorb-dds.

mod error;
pub mod frag;
mod message;
mod parameter;
pub mod pl;
pub mod qos;
pub mod receiver;
pub mod reliable;
pub mod sedp;
pub mod spdp;
mod submessage;
pub mod typelookup;
mod types;
pub mod udp;
mod wire;
pub mod wlp;

pub use error::{DecodeError, EncodeError};
pub use message::{Header, Message, PROTOCOL_RTPS, Submessages};
pub use parameter::{Parameter, ParameterId, ParameterList};
pub use submessage::{
    AckNack, Checksum, Data, DataFrag, Gap, HeaderExtension, Heartbeat, HeartbeatFrag,
    HeartbeatGroupInfo, InfoDestination, InfoReply, InfoReplyIp4, InfoSource, InfoTimestamp,
    NackFrag, Pad, PayloadKind, SerializedPayload, Submessage, UnknownSubmessage, submessage_id,
};
pub use types::{
    BuiltinEndpointSet, ChangeCount, Count, Duration, EntityId, FragmentNumber, FragmentNumberSet,
    GroupDigest, Guid, GuidPrefix, Locator, LocatorUdpV4, ProtocolVersion, SampleIdentity,
    SequenceNumber, SequenceNumberSet, Time, VendorId, entity_kind,
};
pub use wire::Endianness;
