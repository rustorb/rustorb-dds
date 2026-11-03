//! Communication statuses (DDS 1.4, 2.2.4.1): counts of events on a
//! writer or a reader. Reading a status resets its `*_change` counts.

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
