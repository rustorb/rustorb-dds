//! The QoS of publishers, subscribers, data writers and data readers
//! that can be set for now.

use std::time::Duration;

use rustorb_rtps::qos::ReliabilityKind;
pub use rustorb_rtps::qos::{DurabilityKind, LivelinessKind, QosPolicyId};

/// The HISTORY of a writer or a reader (DDS 1.4, 2.2.3.18): how many
/// samples it keeps.
///
/// The depth applies to each instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum History {
    /// Keep the latest `n` samples (at least 1).
    KeepLast(usize),
    /// Keep every sample: a reader until it is taken, a reliable writer
    /// until every reader has acknowledged it.
    KeepAll,
}

impl Default for History {
    /// `KEEP_LAST` with depth 1, the DDS default.
    fn default() -> Self {
        History::KeepLast(1)
    }
}

/// The RELIABILITY of a writer or a reader (DDS 1.4, 2.2.3.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reliability {
    /// BEST_EFFORT or RELIABLE.
    pub kind: ReliabilityKind,
    /// How long a reliable writer's `write` may wait for room in its
    /// history (KEEP_ALL with `max_samples` reached) before it fails with
    /// [`crate::Error::Timeout`].
    pub max_blocking_time: Duration,
}

impl Reliability {
    /// The default `max_blocking_time`: 100 ms (DDS 1.4, 2.2.3).
    pub const DEFAULT_MAX_BLOCKING_TIME: Duration = Duration::from_millis(100);

    /// RELIABLE, with the default blocking time.
    pub const fn reliable() -> Self {
        Reliability {
            kind: ReliabilityKind::Reliable,
            max_blocking_time: Self::DEFAULT_MAX_BLOCKING_TIME,
        }
    }

    /// BEST_EFFORT.
    pub const fn best_effort() -> Self {
        Reliability {
            kind: ReliabilityKind::BestEffort,
            max_blocking_time: Self::DEFAULT_MAX_BLOCKING_TIME,
        }
    }

    pub(crate) fn to_rtps(self) -> rustorb_rtps::qos::Reliability {
        rustorb_rtps::qos::Reliability {
            kind: self.kind,
            max_blocking_time: rustorb_rtps::Duration::from_std(self.max_blocking_time),
        }
    }
}

/// The RESOURCE_LIMITS of a writer or a reader (DDS 1.4, 2.2.3.19).
/// `None` is `LENGTH_UNLIMITED`, the default.
///
/// A RELIABLE, KEEP_ALL writer waits when it has `max_samples` samples,
/// or `max_samples_per_instance` of the instance it writes; registering
/// an instance beyond `max_instances` fails.
///
/// A reader rejects a sample that would go beyond them (SAMPLE_REJECTED,
/// [`crate::DataReader::sample_rejected_status`]); under KEEP_LAST, a
/// sample that replaces the oldest of its instance is not rejected. A
/// reliable reader acknowledges a rejected sample all the same: it is not
/// sent again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceLimits {
    /// The most samples kept.
    pub max_samples: Option<usize>,
    /// The most instances kept.
    pub max_instances: Option<usize>,
    /// The most samples kept for one instance.
    pub max_samples_per_instance: Option<usize>,
}

/// The WRITER_DATA_LIFECYCLE of a writer (DDS 1.4, 2.2.3.21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterDataLifecycle {
    /// Whether unregistering an instance, which deleting the writer does
    /// for all of them, disposes it too. TRUE by default.
    pub autodispose_unregistered_instances: bool,
}

impl Default for WriterDataLifecycle {
    fn default() -> Self {
        WriterDataLifecycle {
            autodispose_unregistered_instances: true,
        }
    }
}

/// The LIVELINESS of a writer or a reader (DDS 1.4, 2.2.3.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liveliness {
    /// AUTOMATIC, MANUAL_BY_PARTICIPANT or MANUAL_BY_TOPIC.
    pub kind: LivelinessKind,
    /// How long a writer is alive without asserting it; `None` is
    /// infinite, the default.
    pub lease_duration: Option<Duration>,
}

impl Default for Liveliness {
    /// AUTOMATIC with an infinite lease.
    fn default() -> Self {
        Liveliness {
            kind: LivelinessKind::Automatic,
            lease_duration: None,
        }
    }
}

impl Liveliness {
    pub(crate) fn to_rtps(self) -> rustorb_rtps::qos::Liveliness {
        rustorb_rtps::qos::Liveliness {
            kind: self.kind,
            lease_duration: duration_to_rtps(self.lease_duration),
        }
    }
}

/// An optional duration on the wire: `None` is infinite.
pub(crate) fn duration_to_rtps(d: Option<Duration>) -> rustorb_rtps::Duration {
    d.map_or(
        rustorb_rtps::Duration::INFINITE,
        rustorb_rtps::Duration::from_std,
    )
}

/// The QoS of a [`crate::Publisher`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublisherQos {
    /// PARTITION (DDS 1.4, 2.2.3.13): the partitions its writers are in.
    /// Empty is the default partition. A name may hold the fnmatch
    /// wildcards `*`, `?` and `[...]`, which match the names of readers'
    /// partitions (but not other wildcard names).
    pub partition: Vec<String>,
}

/// The QoS of a [`crate::Subscriber`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubscriberQos {
    /// PARTITION: the partitions its readers are in, as for
    /// [`PublisherQos::partition`].
    pub partition: Vec<String>,
}

/// The QoS of a [`crate::DataWriter`] that can be set for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataWriterQos {
    /// RELIABILITY; RELIABLE by default.
    pub reliability: Reliability,
    /// HISTORY; KEEP_LAST(1) by default. The depth is for each instance.
    pub history: History,
    /// RESOURCE_LIMITS.
    pub resource_limits: ResourceLimits,
    /// WRITER_DATA_LIFECYCLE.
    pub writer_data_lifecycle: WriterDataLifecycle,
    /// DURABILITY; VOLATILE by default. A TRANSIENT_LOCAL writer keeps
    /// the samples its HISTORY and RESOURCE_LIMITS allow, also once they
    /// are acknowledged, and sends them to the TRANSIENT_LOCAL readers
    /// matched later; for now it keeps them also for instances it has
    /// unregistered. TRANSIENT and PERSISTENT need a durability service,
    /// which there is not: creating such a writer fails with
    /// [`crate::Error::Unsupported`].
    pub durability: DurabilityKind,
    /// DEADLINE period (DDS 1.4, 2.2.3.7); `None`, infinite, by default.
    /// It is announced and matched on.
    pub deadline: Option<Duration>,
    /// LIVELINESS. It is announced and matched on.
    pub liveliness: Liveliness,
}

impl Default for DataWriterQos {
    /// The DDS defaults: RELIABLE, KEEP_LAST(1), no limits, VOLATILE.
    fn default() -> Self {
        DataWriterQos {
            reliability: Reliability::reliable(),
            history: History::default(),
            resource_limits: ResourceLimits::default(),
            writer_data_lifecycle: WriterDataLifecycle::default(),
            durability: DurabilityKind::Volatile,
            deadline: None,
            liveliness: Liveliness::default(),
        }
    }
}

/// The QoS of a [`crate::DataReader`] that can be set for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataReaderQos {
    /// RELIABILITY; BEST_EFFORT by default.
    pub reliability: Reliability,
    /// HISTORY; KEEP_LAST(1) by default. The depth is for each instance.
    pub history: History,
    /// RESOURCE_LIMITS; none by default.
    pub resource_limits: ResourceLimits,
    /// DURABILITY; VOLATILE by default. A TRANSIENT_LOCAL reader gets
    /// what a TRANSIENT_LOCAL writer kept from before they matched.
    pub durability: DurabilityKind,
    /// DEADLINE period; `None`, infinite, by default. It is announced
    /// and matched on.
    pub deadline: Option<Duration>,
    /// LIVELINESS. It is announced and matched on.
    pub liveliness: Liveliness,
}

impl Default for DataReaderQos {
    /// The DDS defaults: BEST_EFFORT, KEEP_LAST(1), VOLATILE.
    fn default() -> Self {
        DataReaderQos {
            reliability: Reliability::best_effort(),
            history: History::default(),
            resource_limits: ResourceLimits::default(),
            durability: DurabilityKind::Volatile,
            deadline: None,
            liveliness: Liveliness::default(),
        }
    }
}
