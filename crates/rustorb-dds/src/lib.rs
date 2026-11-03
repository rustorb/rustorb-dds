//! A DDS 1.4 implementation in Rust: the DCPS API (domain participants,
//! topics, publishers, subscribers, data writers and data readers) over
//! DDSI-RTPS 2.5 ([`rustorb_rtps`]).
//!
//! Sample types implement [`TopicType`] from `rustorb-typesupport`, which
//! the IDL compiler generates (`rustorb-idl -brust -Wbdds`); a
//! hand-written implementation works the same way.
//!
//! ```no_run
//! # fn f<Greeting: rustorb_dds::TopicType + 'static>(hello: Greeting) -> rustorb_dds::Result<()> {
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
//!         // `None` when the sample only tells of a change in the state
//!         // of its instance (disposed, no more writers).
//!         let _greeting: Option<Greeting> = sample.data;
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! What works now (rustorb/rustorb#111, stages D1 and part of D2):
//! discovery with SPDP and SEDP, and RELIABLE or BEST_EFFORT, VOLATILE or
//! TRANSIENT_LOCAL writers and readers, in partitions (with wildcards),
//! over UDPv4. Endpoints whose QoS do not match are counted in the
//! incompatible QoS statuses; readers reject samples beyond their
//! RESOURCE_LIMITS. A participant that is deleted tells the others. Keyed types
//! have instances: writers register, write, dispose and unregister them;
//! readers keep a history per instance with its states (DDS 1.4,
//! 2.2.2.5.1). Samples larger than a datagram go as DATA_FRAG, and lost
//! fragments are repaired. Samples are written in XCDR1. Writers and
//! readers in the same participant are matched with each other too.
//!
//! Still to come in D2 (rustorb-dds#15): LIVELINESS and DEADLINE
//! behaviour (they are announced and matched on already), WaitSets, ROS 2;
//! then the XTypes type information (D3).

mod error;
mod instance;
mod participant;
mod publisher;
mod qos;
mod status;
mod subscriber;
#[cfg(test)]
#[path = "../examples/keyed/shape.rs"]
mod test_shape;
mod topic;
mod typed;

pub use error::{Error, Result};
pub use instance::{
    InstanceHandle, InstanceState, InstanceStateMask, SampleInfo, SampleState, SampleStateMask,
    StateFilter, ViewState, ViewStateMask,
};
pub use participant::{DomainParticipant, ParticipantConfig, VENDOR_ID};
pub use publisher::{DataWriter, Publisher};
pub use qos::{
    DataReaderQos, DataWriterQos, DurabilityKind, History, Liveliness, LivelinessKind,
    PublisherQos, QosPolicyId, Reliability, ResourceLimits, SubscriberQos, WriterDataLifecycle,
};
pub use rustorb_rtps::qos::ReliabilityKind;
pub use rustorb_rtps::{Guid, GuidPrefix, SequenceNumber, Time};
pub use rustorb_typesupport::TopicType;
pub use status::{
    IncompatibleQosStatus, QosPolicyCount, SampleRejectedReason, SampleRejectedStatus,
};
pub use subscriber::{DataReader, Sample, Subscriber};
pub use topic::Topic;
