//! The QoS of data writers and data readers that can be set for now.

use std::time::Duration;

use rustorb_rtps::qos::ReliabilityKind;

/// The HISTORY of a writer or a reader (DDS 1.4, 2.2.3.18): how many
/// samples it keeps.
///
/// For now the depth applies to the whole writer or reader, not to each
/// instance (rustorb-dds#17).
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
/// For now only `max_samples` is used, by writers with KEEP_ALL history.
/// The others come with instances and the rest of the QoS
/// (rustorb-dds#17, #19).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceLimits {
    /// The most samples kept.
    pub max_samples: Option<usize>,
    /// The most instances kept.
    pub max_instances: Option<usize>,
    /// The most samples kept for one instance.
    pub max_samples_per_instance: Option<usize>,
}

/// The QoS of a [`crate::DataWriter`] that can be set for now. The
/// writer is VOLATILE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataWriterQos {
    /// RELIABILITY; RELIABLE by default.
    pub reliability: Reliability,
    /// HISTORY; KEEP_LAST(1) by default.
    pub history: History,
    /// RESOURCE_LIMITS.
    pub resource_limits: ResourceLimits,
}

impl Default for DataWriterQos {
    /// The DDS defaults: RELIABLE, KEEP_LAST(1), no limits.
    fn default() -> Self {
        DataWriterQos {
            reliability: Reliability::reliable(),
            history: History::default(),
            resource_limits: ResourceLimits::default(),
        }
    }
}

/// The QoS of a [`crate::DataReader`] that can be set for now. The
/// reader is VOLATILE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataReaderQos {
    /// RELIABILITY; BEST_EFFORT by default.
    pub reliability: Reliability,
    /// HISTORY; KEEP_LAST(1) by default.
    pub history: History,
}

impl Default for DataReaderQos {
    /// The DDS defaults: BEST_EFFORT, KEEP_LAST(1).
    fn default() -> Self {
        DataReaderQos {
            reliability: Reliability::best_effort(),
            history: History::default(),
        }
    }
}
