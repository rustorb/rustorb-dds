//! A DDS 1.4 implementation in Rust: the DCPS API (domain participants,
//! topics, publishers, subscribers, data writers and data readers) over
//! DDSI-RTPS 2.5 ([`rustorb_rtps`]).
//!
//! Sample types implement [`TopicType`] from `rustorb-typesupport`, which
//! the IDL compiler generates (`rustorb-idl -brust -Wbdds`); a
//! hand-written implementation works the same way.
//!
//! ```no_run
//! # fn f<Greeting: rustorb_dds::TopicType>(hello: Greeting) -> rustorb_dds::Result<()> {
//! use std::time::Duration;
//! use rustorb_dds::DomainParticipant;
//!
//! let participant = DomainParticipant::new(0)?;
//! let topic = participant.create_topic::<Greeting>("hello")?;
//! let writer = participant.create_publisher()?.create_datawriter(&topic)?;
//! let reader = participant.create_subscriber()?.create_datareader(&topic)?;
//! writer.write(&hello)?;
//! if reader.wait_for_data(Duration::from_secs(1)) {
//!     for sample in reader.take() {
//!         let _greeting: Greeting = sample.data;
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! What works now (rustorb/rustorb#111, stage D1): discovery with SPDP
//! and SEDP, and BEST_EFFORT, VOLATILE writers and readers in the default
//! partition, over UDPv4. Samples are written in XCDR1. Writers and
//! readers in the same participant are matched with each other too.
//!
//! Still to come: dispose and unregister, instances, more QoS (reliable
//! delivery and durability in D2), partitions in the API, DATA_FRAG for
//! samples larger than a datagram, and the XTypes type information (D3).

mod error;
mod participant;
mod publisher;
mod subscriber;
mod topic;
mod wire;

pub use error::{Error, Result};
pub use participant::{DomainParticipant, ParticipantConfig, VENDOR_ID};
pub use publisher::{DataWriter, Publisher};
pub use rustorb_rtps::{Guid, GuidPrefix, SequenceNumber, Time};
pub use rustorb_typesupport::TopicType;
pub use subscriber::{DataReader, DataReaderQos, History, Sample, SampleInfo, Subscriber};
pub use topic::Topic;
