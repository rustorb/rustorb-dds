//! Communication statuses (DDS 1.4, 2.2.4.1): counts of events on a
//! writer or a reader. Reading a status resets its `*_change` counts.

use rustorb_rtps::Guid;
use rustorb_rtps::qos::QosPolicyId;

use crate::instance::InstanceHandle;

/// How many times one policy made endpoints incompatible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QosPolicyCount {
    /// The policy.
    pub policy_id: QosPolicyId,
    /// How many times.
    pub count: u32,
}

/// OFFERED_INCOMPATIBLE_QOS of a writer or REQUESTED_INCOMPATIBLE_QOS of
/// a reader (DDS 1.4, 2.2.4.1): endpoints of the same topic, type and
/// partition that did not match because of their QoS.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncompatibleQosStatus {
    /// How many such endpoints were found.
    pub total_count: u32,
    /// How many since the status was last read.
    pub total_count_change: u32,
    /// A policy that was incompatible the last time.
    pub last_policy_id: Option<QosPolicyId>,
    /// How many times each policy was incompatible.
    pub policies: Vec<QosPolicyCount>,
}

impl IncompatibleQosStatus {
    /// Counts one incompatible endpoint, for the given policies.
    pub(crate) fn add(&mut self, policies: &[QosPolicyId]) {
        self.total_count += 1;
        self.total_count_change += 1;
        self.last_policy_id = policies.first().copied();
        for id in policies {
            match self.policies.iter_mut().find(|c| c.policy_id == *id) {
                Some(c) => c.count += 1,
                None => self.policies.push(QosPolicyCount {
                    policy_id: *id,
                    count: 1,
                }),
            }
        }
    }

    /// Returns the status and resets the change count.
    pub(crate) fn take(&mut self) -> Self {
        let s = self.clone();
        self.total_count_change = 0;
        s
    }
}

/// Why a reader rejected a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleRejectedReason {
    /// `max_instances` was reached.
    InstancesLimit,
    /// `max_samples` was reached.
    SamplesLimit,
    /// `max_samples_per_instance` was reached.
    SamplesPerInstanceLimit,
}

/// SAMPLE_REJECTED of a reader (DDS 1.4, 2.2.4.1): samples dropped
/// because of its RESOURCE_LIMITS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleRejectedStatus {
    /// How many samples were rejected.
    pub total_count: u32,
    /// How many since the status was last read.
    pub total_count_change: u32,
    /// Why the last one was.
    pub last_reason: Option<SampleRejectedReason>,
    /// The instance of the last one.
    pub last_instance_handle: Option<InstanceHandle>,
}

/// LIVELINESS_LOST of a writer (DDS 1.4, 2.2.4.1): times it failed to
/// assert its liveliness within its lease, so that readers took it as no
/// longer alive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LivelinessLostStatus {
    /// How many times it became not alive.
    pub total_count: u32,
    /// How many since the status was last read.
    pub total_count_change: u32,
}

impl LivelinessLostStatus {
    pub(crate) fn add(&mut self) {
        self.total_count += 1;
        self.total_count_change += 1;
    }

    pub(crate) fn take(&mut self) -> Self {
        let s = *self;
        self.total_count_change = 0;
        s
    }
}

/// LIVELINESS_CHANGED of a reader (DDS 1.4, 2.2.4.1): how many of its
/// matched writers are alive, and how many are not.
///
/// A writer counts as alive once it is matched. It becomes not alive
/// when its lease passes without it asserting its liveliness, and alive
/// again when it asserts it. A writer that goes away is no longer
/// counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LivelinessChangedStatus {
    /// The matched writers alive now.
    pub alive_count: u32,
    /// The matched writers not alive now.
    pub not_alive_count: u32,
    /// The change in `alive_count` since the status was last read.
    pub alive_count_change: i32,
    /// The change in `not_alive_count` since the status was last read.
    pub not_alive_count_change: i32,
    /// The last writer whose liveliness changed.
    pub last_publication_handle: Option<Guid>,
}

impl LivelinessChangedStatus {
    /// A writer is matched (`Some(true)`, alive), not alive any more
    /// (`Some(false)`), or gone (`None`), having been alive or not.
    pub(crate) fn update(&mut self, writer: Guid, was: Option<bool>, now: Option<bool>) {
        let count = |s: &mut Self, alive: bool, d: i32| {
            if alive {
                s.alive_count = s.alive_count.saturating_add_signed(d);
                s.alive_count_change += d;
            } else {
                s.not_alive_count = s.not_alive_count.saturating_add_signed(d);
                s.not_alive_count_change += d;
            }
        };
        if was == now {
            return;
        }
        if let Some(a) = was {
            count(self, a, -1);
        }
        if let Some(a) = now {
            count(self, a, 1);
        }
        self.last_publication_handle = Some(writer);
    }

    pub(crate) fn take(&mut self) -> Self {
        let s = *self;
        self.alive_count_change = 0;
        self.not_alive_count_change = 0;
        s
    }
}

/// OFFERED_DEADLINE_MISSED of a writer or REQUESTED_DEADLINE_MISSED of a
/// reader (DDS 1.4, 2.2.4.1): DEADLINE periods that passed without a
/// sample of an instance, one count for each period and instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeadlineMissedStatus {
    /// How many deadlines were missed.
    pub total_count: u32,
    /// How many since the status was last read.
    pub total_count_change: u32,
    /// The instance of the last one.
    pub last_instance_handle: Option<InstanceHandle>,
}

impl DeadlineMissedStatus {
    pub(crate) fn take(&mut self) -> Self {
        let s = *self;
        self.total_count_change = 0;
        s
    }
}

/// Deadlines of instances: when each must next get a sample.
#[derive(Debug, Default)]
pub(crate) struct Deadlines {
    period: Option<std::time::Duration>,
    due: std::collections::BTreeMap<InstanceHandle, std::time::Instant>,
    pub(crate) missed: DeadlineMissedStatus,
}

impl Deadlines {
    pub(crate) fn new(period: Option<std::time::Duration>) -> Self {
        Deadlines {
            period,
            ..Self::default()
        }
    }

    /// The instance got a sample at `now`.
    pub(crate) fn renew(&mut self, h: InstanceHandle, now: std::time::Instant) {
        if let Some(p) = self.period {
            self.due.insert(h, now + p);
        }
    }

    /// The instance is no longer expected to get samples.
    pub(crate) fn remove(&mut self, h: &InstanceHandle) {
        self.due.remove(h);
    }

    /// Counts the periods that passed by `now` without a sample, for the
    /// instances `keep` keeps; the others are dropped.
    pub(crate) fn check(
        &mut self,
        now: std::time::Instant,
        mut keep: impl FnMut(&InstanceHandle) -> bool,
    ) {
        let Some(p) = self.period else {
            return;
        };
        self.due.retain(|h, _| keep(h));
        for (h, due) in self.due.iter_mut() {
            while *due <= now {
                self.missed.total_count += 1;
                self.missed.total_count_change += 1;
                self.missed.last_instance_handle = Some(*h);
                *due += p.max(std::time::Duration::from_millis(1));
            }
        }
    }
}
