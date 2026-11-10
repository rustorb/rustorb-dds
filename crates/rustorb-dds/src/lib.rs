//! A DDS 1.4 implementation in Rust: the DCPS API (domain participants,
//! topics, publishers, subscribers, data writers and data readers) over
//! DDSI-RTPS 2.5 ([`rustorb_rtps`]).
//!
//! Sample types implement [`TopicType`] from `rustorb-typesupport`, which
//! the IDL compiler generates (`rustorb-idl -brust -Wbdds`); a
//! hand-written implementation works the same way. Samples of types known
//! only at run time are [`DynamicData`].
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
//! RESOURCE_LIMITS. LIVELINESS (with the Writer Liveliness Protocol) and
//! DEADLINE are kept and reported in statuses. A thread can wait for
//! samples or status changes on a [`WaitSet`], with a [`StatusCondition`],
//! a [`ReadCondition`] or a [`GuardCondition`], or have them handed to a
//! [`DataReaderListener`] or [`DataWriterListener`]. A participant that is
//! deleted tells the others. Keyed types
//! have instances: writers register, write, dispose and unregister them;
//! readers keep a history per instance with its states (DDS 1.4,
//! 2.2.2.5.1). Samples larger than a datagram go as DATA_FRAG, and lost
//! fragments are repaired. Samples are written in XCDR1. Writers and
//! readers in the same participant are matched with each other too.
//!
//! Data flows with ROS 2 nodes too (the `ros2` example).
//!
//! A [`ContentFilteredTopic`] (DDS 1.4, 2.2.2.3.3) gives its readers only
//! the samples of its related topic that pass a filter expression
//! (DDS 1.4, Annex B, see [`ContentFilter`]); readers are created for a
//! [`TopicDescription`], a topic or a content filtered topic.
//!
//! DDS-XTypes (D3, in progress): a type registered with
//! [`DomainParticipant::register_type`] (its [`DescribeType`] is written
//! by `rustorb-idl -Wbxtypes`) is announced with its TypeInformation, and
//! its endpoints match others by type assignability
//! ([`DataReaderQos::type_consistency`]) instead of by type name. Writers
//! can write XCDR2 ([`DataWriterQos::data_representation`]). Topics count
//! the endpoints whose types are not consistent with theirs
//! ([`Topic::inconsistent_topic_status`]). When a remote endpoint's type
//! differs from the local one, the participant gets its TypeObjects from
//! the remote participant with the TypeLookup service (DDS-XTypes 1.3,
//! 7.6.3.3) before it decides, and answers such requests for its own
//! types ([`DomainParticipant::learnt_types`], the `evolve` example).
//!
//! A program can also handle topics whose types it was not built with
//! (DDS-XTypes 1.3, 7.5.2): [`DomainParticipant::discover_topic_type`]
//! gets the type of a remote endpoint of a topic as a [`DynamicType`],
//! by TypeLookup, and [`DomainParticipant::create_dynamic_topic`] makes a
//! topic of it (or of a type built at run time) whose writers and readers
//! write and read [`DynamicData`] (the `dynamic` example).

mod condition;
mod error;
mod instance;
mod listener;
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

pub use condition::{Condition, GuardCondition, ReadCondition, StatusCondition, WaitSet};
pub use error::{Error, Result};
pub use instance::{
    InstanceHandle, InstanceState, InstanceStateMask, SampleInfo, SampleState, SampleStateMask,
    StateFilter, ViewState, ViewStateMask,
};
pub use listener::{DataReaderListener, DataWriterListener};
pub use participant::{DomainParticipant, ParticipantConfig, VENDOR_ID};
pub use publisher::{DataWriter, Publisher};
pub use qos::{
    DataReaderQos, DataWriterQos, DurabilityKind, History, Liveliness, LivelinessKind,
    PublisherQos, QosPolicyId, Reliability, ResourceLimits, SubscriberQos, TypeConsistency,
    TypeConsistencyKind, WriterDataLifecycle, data_representation,
};
pub use rustorb_rtps::qos::ReliabilityKind;
pub use rustorb_rtps::sedp::DATA_REPRESENTATION_QOS_POLICY_ID;
pub use rustorb_rtps::{Guid, GuidPrefix, SequenceNumber, Time};
pub use rustorb_typesupport::TopicType;
pub use rustorb_xtypes::dynamic::ContentFilter;
pub use rustorb_xtypes::dynamic::{
    DynamicData, DynamicDataFactory, DynamicType, DynamicTypeBuilder, DynamicTypeBuilderFactory,
    MEMBER_ID_INVALID,
};
pub use rustorb_xtypes::types::DescribeType;
pub use status::{
    DeadlineMissedStatus, IncompatibleQosStatus, InconsistentTopicStatus, LivelinessChangedStatus,
    LivelinessLostStatus, MatchedStatus, QosPolicyCount, SampleLostStatus, SampleRejectedReason,
    SampleRejectedStatus, StatusMask,
};
pub use subscriber::{DataReader, Sample, Subscriber};
pub use topic::{ContentFilteredTopic, Topic, TopicDescription};
