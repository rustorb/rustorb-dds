//! The DCPS entities (`rdds_participant`, `rdds_topic`,
//! `rdds_publisher`, `rdds_subscriber`, `rdds_writer`, `rdds_reader`),
//! their QoS, statuses and listeners, and the samples a reader returns
//! (`rdds_samples`).

use std::ffi::{c_char, c_void};
use std::time::Duration;

use rustorb_dds::{
    ContentFilteredTopic, DataReader, DataReaderListener, DataReaderQos, DataWriter,
    DataWriterListener, DataWriterQos, DeadlineMissedStatus, DomainParticipant, DurabilityKind,
    DynamicData, DynamicType, Guid, History, IncompatibleQosStatus, InconsistentTopicStatus,
    InstanceHandle, InstanceState, InstanceStateMask, Liveliness, LivelinessChangedStatus,
    LivelinessKind, LivelinessLostStatus, MatchedStatus, ParticipantConfig, Publisher,
    PublisherQos, Reliability, ReliabilityKind, ResourceLimits, Sample, SampleLostStatus,
    SampleRejectedReason, SampleRejectedStatus, SampleState, SampleStateMask, StateFilter,
    StatusMask, Subscriber, SubscriberQos, Time, Topic, TypeConsistency, TypeConsistencyKind,
    ViewState, ViewStateMask, data_representation,
};

use crate::condition::RddsCondition;
use crate::stream::{In, Out, from_data, to_data};
use crate::{Res, Status, TIMEOUT, bad, cstr, guard, guard_value, obj, put, release, set, slice};

// ---------- plain values --------------------------------------------------

/// `rdds_duration` (DDS `Duration_t`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RddsDuration {
    pub sec: i32,
    pub nanosec: u32,
}

/// `DURATION_INFINITE_SEC` and `DURATION_INFINITE_NSEC`.
pub const DURATION_INFINITE: RddsDuration = RddsDuration {
    sec: 0x7fff_ffff,
    nanosec: 0x7fff_ffff,
};

impl RddsDuration {
    fn to_std(self) -> Res<Option<Duration>> {
        if self == DURATION_INFINITE {
            return Ok(None);
        }
        if self.sec < 0 || self.nanosec >= 1_000_000_000 {
            return Err(bad("a duration out of range"));
        }
        Ok(Some(Duration::new(self.sec as u64, self.nanosec)))
    }

    fn finite(self) -> Res<Duration> {
        // An infinite wait is as long as anybody waits.
        Ok(self
            .to_std()?
            .unwrap_or(Duration::from_secs(u32::MAX.into())))
    }

    fn of(d: Option<Duration>) -> RddsDuration {
        match d {
            None => DURATION_INFINITE,
            Some(d) => RddsDuration {
                sec: d.as_secs().min(0x7fff_fffe) as i32,
                nanosec: d.subsec_nanos(),
            },
        }
    }
}

/// `rdds_time` (DDS `Time_t`): since 1970.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RddsTime {
    pub sec: i32,
    pub nanosec: u32,
}

/// `TIME_INVALID`.
pub const TIME_INVALID: RddsTime = RddsTime {
    sec: -1,
    nanosec: 0xffff_ffff,
};

impl RddsTime {
    fn to_rtps(self) -> Res<Time> {
        if self.sec < 0 || self.nanosec >= 1_000_000_000 {
            return Err(bad("a timestamp out of range"));
        }
        Ok(Time {
            seconds: self.sec as u32,
            fraction: ((u64::from(self.nanosec) << 32) / 1_000_000_000) as u32,
        })
    }

    fn of(t: Option<Time>) -> RddsTime {
        match t {
            None => TIME_INVALID,
            Some(t) => RddsTime {
                sec: t.seconds.min(0x7fff_ffff) as i32,
                // Rounded up, so that a time to_rtps made comes back
                // to the same nanosecond.
                nanosec: (((u64::from(t.fraction) * 1_000_000_000) + 0xffff_ffff) >> 32)
                    .min(999_999_999) as u32,
            },
        }
    }
}

unsafe fn timestamp(t: *const RddsTime) -> Res<Time> {
    match unsafe { t.as_ref() } {
        None => Ok(Time::now()),
        Some(t) => t.to_rtps(),
    }
}

/// `rdds_guid`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RddsGuid {
    pub value: [u8; 16],
}

impl RddsGuid {
    fn of(g: Option<Guid>) -> RddsGuid {
        RddsGuid {
            value: g.map_or([0; 16], Guid::to_bytes),
        }
    }
}

/// `rdds_instance_handle`: zeros for `HANDLE_NIL`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RddsInstanceHandle {
    pub value: [u8; 16],
}

impl RddsInstanceHandle {
    fn of(h: Option<InstanceHandle>) -> RddsInstanceHandle {
        RddsInstanceHandle {
            value: h.map_or([0; 16], |h| h.0),
        }
    }
}

// ---------- QoS -----------------------------------------------------------

/// `RDDS_LENGTH_UNLIMITED`.
pub const LENGTH_UNLIMITED: i32 = -1;

/// `rdds_writer_qos`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsWriterQos {
    /// `RDDS_BEST_EFFORT_RELIABILITY_QOS` (0) or
    /// `RDDS_RELIABLE_RELIABILITY_QOS` (1).
    pub reliability_kind: i32,
    pub max_blocking_time: RddsDuration,
    /// `RDDS_KEEP_LAST_HISTORY_QOS` (0) or `RDDS_KEEP_ALL_HISTORY_QOS` (1).
    pub history_kind: i32,
    pub history_depth: i32,
    pub max_samples: i32,
    pub max_instances: i32,
    pub max_samples_per_instance: i32,
    /// `RDDS_VOLATILE_DURABILITY_QOS` (0) ... `RDDS_PERSISTENT_DURABILITY_QOS` (3).
    pub durability_kind: i32,
    pub deadline_period: RddsDuration,
    /// `RDDS_AUTOMATIC_LIVELINESS_QOS` (0) ... `RDDS_MANUAL_BY_TOPIC_LIVELINESS_QOS` (2).
    pub liveliness_kind: i32,
    pub lease_duration: RddsDuration,
    pub user_data: *const u8,
    pub user_data_len: usize,
    /// Data representation ids (`XCDR_DATA_REPRESENTATION` 0,
    /// `XCDR2_DATA_REPRESENTATION` 2); the first is written. None for
    /// the default (XCDR).
    pub data_representation: *const i16,
    pub data_representation_len: usize,
    pub autodispose_unregistered_instances: bool,
}

/// `rdds_reader_qos`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsReaderQos {
    pub reliability_kind: i32,
    pub max_blocking_time: RddsDuration,
    pub history_kind: i32,
    pub history_depth: i32,
    pub max_samples: i32,
    pub max_instances: i32,
    pub max_samples_per_instance: i32,
    pub durability_kind: i32,
    pub deadline_period: RddsDuration,
    pub liveliness_kind: i32,
    pub lease_duration: RddsDuration,
    pub user_data: *const u8,
    pub user_data_len: usize,
    pub data_representation: *const i16,
    pub data_representation_len: usize,
    /// `RDDS_DISALLOW_TYPE_COERCION` (0) or `RDDS_ALLOW_TYPE_COERCION` (1).
    pub type_consistency_kind: i32,
    pub ignore_sequence_bounds: bool,
    pub ignore_string_bounds: bool,
    pub ignore_member_names: bool,
    pub prevent_type_widening: bool,
    pub force_type_validation: bool,
}

/// `rdds_publisher_qos` and `rdds_subscriber_qos`: the PARTITION names.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsGroupQos {
    pub partition: *const *const c_char,
    pub partition_len: usize,
}

/// `rdds_participant_qos`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsParticipantQos {
    pub user_data: *const u8,
    pub user_data_len: usize,
}

static XCDR_BOTH: [i16; 2] = [data_representation::XCDR, data_representation::XCDR2];

struct Common {
    reliability: Reliability,
    history: History,
    resource_limits: ResourceLimits,
    durability: DurabilityKind,
    deadline: Option<Duration>,
    liveliness: Liveliness,
    user_data: Vec<u8>,
    data_representation: Vec<i16>,
}

fn limit(v: i32) -> Res<Option<usize>> {
    match v {
        LENGTH_UNLIMITED => Ok(None),
        v if v > 0 => Ok(Some(v as usize)),
        _ => Err(bad(format!("a resource limit of {v}"))),
    }
}

fn unlimited(v: Option<usize>) -> i32 {
    v.map_or(LENGTH_UNLIMITED, |v| v.min(i32::MAX as usize) as i32)
}

#[allow(clippy::too_many_arguments)]
unsafe fn common(
    reliability_kind: i32,
    max_blocking_time: RddsDuration,
    history_kind: i32,
    history_depth: i32,
    limits: [i32; 3],
    durability_kind: i32,
    deadline: RddsDuration,
    liveliness_kind: i32,
    lease_duration: RddsDuration,
    user_data: (*const u8, usize),
    representation: (*const i16, usize),
) -> Res<Common> {
    let kind = match reliability_kind {
        0 => ReliabilityKind::BestEffort,
        1 => ReliabilityKind::Reliable,
        k => return Err(bad(format!("unknown reliability kind {k}"))),
    };
    let reliability = Reliability {
        kind,
        max_blocking_time: max_blocking_time.finite()?,
    };
    let history = match history_kind {
        0 if history_depth > 0 => History::KeepLast(history_depth as usize),
        0 => return Err(bad("a KEEP_LAST history of depth below 1")),
        1 => History::KeepAll,
        k => return Err(bad(format!("unknown history kind {k}"))),
    };
    let durability = match durability_kind {
        0 => DurabilityKind::Volatile,
        1 => DurabilityKind::TransientLocal,
        2 => DurabilityKind::Transient,
        3 => DurabilityKind::Persistent,
        k => return Err(bad(format!("unknown durability kind {k}"))),
    };
    let kind = match liveliness_kind {
        0 => LivelinessKind::Automatic,
        1 => LivelinessKind::ManualByParticipant,
        2 => LivelinessKind::ManualByTopic,
        k => return Err(bad(format!("unknown liveliness kind {k}"))),
    };
    Ok(Common {
        reliability,
        history,
        resource_limits: ResourceLimits {
            max_samples: limit(limits[0])?,
            max_instances: limit(limits[1])?,
            max_samples_per_instance: limit(limits[2])?,
        },
        durability,
        deadline: deadline.to_std()?,
        liveliness: Liveliness {
            kind,
            lease_duration: lease_duration.to_std()?,
        },
        user_data: unsafe { slice(user_data.0, user_data.1) }?.to_vec(),
        data_representation: unsafe { slice(representation.0, representation.1) }?.to_vec(),
    })
}

unsafe fn writer_qos(q: *const RddsWriterQos) -> Res<DataWriterQos> {
    let Some(q) = (unsafe { q.as_ref() }) else {
        return Ok(DataWriterQos::default());
    };
    let c = unsafe {
        common(
            q.reliability_kind,
            q.max_blocking_time,
            q.history_kind,
            q.history_depth,
            [q.max_samples, q.max_instances, q.max_samples_per_instance],
            q.durability_kind,
            q.deadline_period,
            q.liveliness_kind,
            q.lease_duration,
            (q.user_data, q.user_data_len),
            (q.data_representation, q.data_representation_len),
        )
    }?;
    let mut qos = DataWriterQos {
        reliability: c.reliability,
        history: c.history,
        resource_limits: c.resource_limits,
        durability: c.durability,
        deadline: c.deadline,
        liveliness: c.liveliness,
        user_data: c.user_data,
        data_representation: c.data_representation,
        ..DataWriterQos::default()
    };
    qos.writer_data_lifecycle.autodispose_unregistered_instances =
        q.autodispose_unregistered_instances;
    Ok(qos)
}

unsafe fn reader_qos(q: *const RddsReaderQos) -> Res<DataReaderQos> {
    let Some(q) = (unsafe { q.as_ref() }) else {
        return Ok(DataReaderQos::default());
    };
    let c = unsafe {
        common(
            q.reliability_kind,
            q.max_blocking_time,
            q.history_kind,
            q.history_depth,
            [q.max_samples, q.max_instances, q.max_samples_per_instance],
            q.durability_kind,
            q.deadline_period,
            q.liveliness_kind,
            q.lease_duration,
            (q.user_data, q.user_data_len),
            (q.data_representation, q.data_representation_len),
        )
    }?;
    let kind = match q.type_consistency_kind {
        0 => TypeConsistencyKind::DisallowTypeCoercion,
        1 => TypeConsistencyKind::AllowTypeCoercion,
        k => return Err(bad(format!("unknown type consistency kind {k}"))),
    };
    let mut qos = DataReaderQos {
        reliability: c.reliability,
        history: c.history,
        resource_limits: c.resource_limits,
        durability: c.durability,
        deadline: c.deadline,
        liveliness: c.liveliness,
        user_data: c.user_data,
        type_consistency: TypeConsistency {
            kind,
            ignore_sequence_bounds: q.ignore_sequence_bounds,
            ignore_string_bounds: q.ignore_string_bounds,
            ignore_member_names: q.ignore_member_names,
            prevent_type_widening: q.prevent_type_widening,
            force_type_validation: q.force_type_validation,
        },
        ..DataReaderQos::default()
    };
    if !c.data_representation.is_empty() {
        qos.data_representation = c.data_representation;
    }
    Ok(qos)
}

unsafe fn partition(q: *const RddsGroupQos) -> Res<Vec<String>> {
    let Some(q) = (unsafe { q.as_ref() }) else {
        return Ok(Vec::new());
    };
    unsafe { slice(q.partition, q.partition_len) }?
        .iter()
        .map(|p| Ok(unsafe { cstr(*p, "a partition name") }?.to_owned()))
        .collect()
}

fn history_parts(h: History) -> (i32, i32) {
    match h {
        History::KeepLast(n) => (0, n.min(i32::MAX as usize) as i32),
        History::KeepAll => (1, 1),
    }
}

fn durability_kind(d: DurabilityKind) -> i32 {
    match d {
        DurabilityKind::Volatile => 0,
        DurabilityKind::TransientLocal => 1,
        DurabilityKind::Transient => 2,
        DurabilityKind::Persistent => 3,
    }
}

fn liveliness_kind(k: LivelinessKind) -> i32 {
    match k {
        LivelinessKind::Automatic => 0,
        LivelinessKind::ManualByParticipant => 1,
        LivelinessKind::ManualByTopic => 2,
    }
}

fn reliability_kind(k: ReliabilityKind) -> i32 {
    match k {
        ReliabilityKind::BestEffort => 0,
        ReliabilityKind::Reliable => 1,
    }
}

/// `rdds_writer_qos_default`: the default writer QoS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_qos_default(out: *mut RddsWriterQos) -> Status {
    guard(|| {
        let d = DataWriterQos::default();
        let (history_kind, history_depth) = history_parts(d.history);
        let q = RddsWriterQos {
            reliability_kind: reliability_kind(d.reliability.kind),
            max_blocking_time: RddsDuration::of(Some(d.reliability.max_blocking_time)),
            history_kind,
            history_depth,
            max_samples: unlimited(d.resource_limits.max_samples),
            max_instances: unlimited(d.resource_limits.max_instances),
            max_samples_per_instance: unlimited(d.resource_limits.max_samples_per_instance),
            durability_kind: durability_kind(d.durability),
            deadline_period: RddsDuration::of(d.deadline),
            liveliness_kind: liveliness_kind(d.liveliness.kind),
            lease_duration: RddsDuration::of(d.liveliness.lease_duration),
            user_data: std::ptr::null(),
            user_data_len: 0,
            data_representation: std::ptr::null(),
            data_representation_len: 0,
            autodispose_unregistered_instances: d
                .writer_data_lifecycle
                .autodispose_unregistered_instances,
        };
        unsafe { set(out, q) }
    })
}

/// `rdds_reader_qos_default`: the default reader QoS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_qos_default(out: *mut RddsReaderQos) -> Status {
    guard(|| {
        let d = DataReaderQos::default();
        let (history_kind, history_depth) = history_parts(d.history);
        let tc = d.type_consistency;
        let q = RddsReaderQos {
            reliability_kind: reliability_kind(d.reliability.kind),
            max_blocking_time: RddsDuration::of(Some(d.reliability.max_blocking_time)),
            history_kind,
            history_depth,
            max_samples: unlimited(d.resource_limits.max_samples),
            max_instances: unlimited(d.resource_limits.max_instances),
            max_samples_per_instance: unlimited(d.resource_limits.max_samples_per_instance),
            durability_kind: durability_kind(d.durability),
            deadline_period: RddsDuration::of(d.deadline),
            liveliness_kind: liveliness_kind(d.liveliness.kind),
            lease_duration: RddsDuration::of(d.liveliness.lease_duration),
            user_data: std::ptr::null(),
            user_data_len: 0,
            data_representation: XCDR_BOTH.as_ptr(),
            data_representation_len: XCDR_BOTH.len(),
            type_consistency_kind: match tc.kind {
                TypeConsistencyKind::DisallowTypeCoercion => 0,
                _ => 1,
            },
            ignore_sequence_bounds: tc.ignore_sequence_bounds,
            ignore_string_bounds: tc.ignore_string_bounds,
            ignore_member_names: tc.ignore_member_names,
            prevent_type_widening: tc.prevent_type_widening,
            force_type_validation: tc.force_type_validation,
        };
        unsafe { set(out, q) }
    })
}

// ---------- statuses ------------------------------------------------------

/// `rdds_matched_status` (publication or subscription matched).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsMatchedStatus {
    pub total_count: i32,
    pub total_count_change: i32,
    pub current_count: i32,
    pub current_count_change: i32,
    pub last_handle: RddsGuid,
}

impl From<MatchedStatus> for RddsMatchedStatus {
    fn from(s: MatchedStatus) -> Self {
        RddsMatchedStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
            current_count: s.current_count as i32,
            current_count_change: s.current_count_change,
            last_handle: RddsGuid::of(s.last_handle),
        }
    }
}

/// `rdds_qos_policy_count`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsQosPolicyCount {
    pub policy_id: i32,
    pub count: i32,
}

/// The most policies an incompatible-QoS status lists.
pub const MAX_POLICIES: usize = 32;

/// `rdds_incompatible_qos_status` (offered or requested).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsIncompatibleQosStatus {
    pub total_count: i32,
    pub total_count_change: i32,
    pub last_policy_id: i32,
    pub policies_len: u32,
    pub policies: [RddsQosPolicyCount; MAX_POLICIES],
}

impl From<IncompatibleQosStatus> for RddsIncompatibleQosStatus {
    fn from(s: IncompatibleQosStatus) -> Self {
        let mut out = RddsIncompatibleQosStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
            last_policy_id: s.last_policy_id.map_or(0, |p| p.0 as i32),
            ..Default::default()
        };
        for (slot, p) in out.policies.iter_mut().zip(&s.policies) {
            *slot = RddsQosPolicyCount {
                policy_id: p.policy_id.0 as i32,
                count: p.count as i32,
            };
            out.policies_len += 1;
        }
        out
    }
}

/// `rdds_liveliness_changed_status`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsLivelinessChangedStatus {
    pub alive_count: i32,
    pub not_alive_count: i32,
    pub alive_count_change: i32,
    pub not_alive_count_change: i32,
    pub last_publication_handle: RddsGuid,
}

impl From<LivelinessChangedStatus> for RddsLivelinessChangedStatus {
    fn from(s: LivelinessChangedStatus) -> Self {
        RddsLivelinessChangedStatus {
            alive_count: s.alive_count as i32,
            not_alive_count: s.not_alive_count as i32,
            alive_count_change: s.alive_count_change,
            not_alive_count_change: s.not_alive_count_change,
            last_publication_handle: RddsGuid::of(s.last_publication_handle),
        }
    }
}

/// `rdds_count_status`: LIVELINESS_LOST, SAMPLE_LOST and
/// INCONSISTENT_TOPIC, which are only counts.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsCountStatus {
    pub total_count: i32,
    pub total_count_change: i32,
}

impl From<LivelinessLostStatus> for RddsCountStatus {
    fn from(s: LivelinessLostStatus) -> Self {
        RddsCountStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
        }
    }
}

impl From<SampleLostStatus> for RddsCountStatus {
    fn from(s: SampleLostStatus) -> Self {
        RddsCountStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
        }
    }
}

impl From<InconsistentTopicStatus> for RddsCountStatus {
    fn from(s: InconsistentTopicStatus) -> Self {
        RddsCountStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
        }
    }
}

/// `rdds_deadline_missed_status` (offered or requested).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsDeadlineMissedStatus {
    pub total_count: i32,
    pub total_count_change: i32,
    pub last_instance_handle: RddsInstanceHandle,
}

impl From<DeadlineMissedStatus> for RddsDeadlineMissedStatus {
    fn from(s: DeadlineMissedStatus) -> Self {
        RddsDeadlineMissedStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
            last_instance_handle: RddsInstanceHandle::of(s.last_instance_handle),
        }
    }
}

/// `rdds_sample_rejected_status`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RddsSampleRejectedStatus {
    pub total_count: i32,
    pub total_count_change: i32,
    /// `NOT_REJECTED` (0), `REJECTED_BY_INSTANCES_LIMIT` (1),
    /// `REJECTED_BY_SAMPLES_LIMIT` (2),
    /// `REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT` (3).
    pub last_reason: i32,
    pub last_instance_handle: RddsInstanceHandle,
}

impl From<SampleRejectedStatus> for RddsSampleRejectedStatus {
    fn from(s: SampleRejectedStatus) -> Self {
        RddsSampleRejectedStatus {
            total_count: s.total_count as i32,
            total_count_change: s.total_count_change as i32,
            last_reason: match s.last_reason {
                None => 0,
                Some(SampleRejectedReason::InstancesLimit) => 1,
                Some(SampleRejectedReason::SamplesLimit) => 2,
                Some(SampleRejectedReason::SamplesPerInstanceLimit) => 3,
            },
            last_instance_handle: RddsInstanceHandle::of(s.last_instance_handle),
        }
    }
}

// ---------- listeners -----------------------------------------------------

/// `rdds_writer_listener`: functions (any may be NULL) called with
/// `self` on a thread of the participant; `release`, if not NULL, when
/// the listener is replaced or the writer is released.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsWriterListener {
    pub self_: *mut c_void,
    pub on_offered_deadline_missed:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsDeadlineMissedStatus)>,
    pub on_offered_incompatible_qos:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsIncompatibleQosStatus)>,
    pub on_liveliness_lost: Option<unsafe extern "C" fn(*mut c_void, *const RddsCountStatus)>,
    pub on_publication_matched: Option<unsafe extern "C" fn(*mut c_void, *const RddsMatchedStatus)>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// `rdds_reader_listener`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsReaderListener {
    pub self_: *mut c_void,
    pub on_requested_deadline_missed:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsDeadlineMissedStatus)>,
    pub on_requested_incompatible_qos:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsIncompatibleQosStatus)>,
    pub on_sample_rejected:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsSampleRejectedStatus)>,
    pub on_liveliness_changed:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsLivelinessChangedStatus)>,
    pub on_data_available: Option<unsafe extern "C" fn(*mut c_void)>,
    pub on_subscription_matched:
        Option<unsafe extern "C" fn(*mut c_void, *const RddsMatchedStatus)>,
    pub on_sample_lost: Option<unsafe extern "C" fn(*mut c_void, *const RddsCountStatus)>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

struct WriterCallbacks(RddsWriterListener);
struct ReaderCallbacks(RddsReaderListener);

// SAFETY: the caller's functions may be called from any thread (the
// header says so of listeners).
unsafe impl Send for WriterCallbacks {}
unsafe impl Sync for WriterCallbacks {}
unsafe impl Send for ReaderCallbacks {}
unsafe impl Sync for ReaderCallbacks {}

impl Drop for WriterCallbacks {
    fn drop(&mut self) {
        if let Some(f) = self.0.release {
            unsafe { f(self.0.self_) }
        }
    }
}

impl Drop for ReaderCallbacks {
    fn drop(&mut self) {
        if let Some(f) = self.0.release {
            unsafe { f(self.0.self_) }
        }
    }
}

macro_rules! call {
    ($l:expr, $f:ident, $s:expr) => {
        if let Some(f) = $l.$f {
            let s = $s;
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                f($l.self_, &s)
            }));
        }
    };
}

impl DataWriterListener<DynamicData> for WriterCallbacks {
    fn on_publication_matched(&self, _: &DataWriter<DynamicData>, s: MatchedStatus) {
        call!(self.0, on_publication_matched, RddsMatchedStatus::from(s));
    }
    fn on_liveliness_lost(&self, _: &DataWriter<DynamicData>, s: LivelinessLostStatus) {
        call!(self.0, on_liveliness_lost, RddsCountStatus::from(s));
    }
    fn on_offered_deadline_missed(&self, _: &DataWriter<DynamicData>, s: DeadlineMissedStatus) {
        call!(
            self.0,
            on_offered_deadline_missed,
            RddsDeadlineMissedStatus::from(s)
        );
    }
    fn on_offered_incompatible_qos(&self, _: &DataWriter<DynamicData>, s: IncompatibleQosStatus) {
        call!(
            self.0,
            on_offered_incompatible_qos,
            RddsIncompatibleQosStatus::from(s)
        );
    }
}

impl DataReaderListener<DynamicData> for ReaderCallbacks {
    fn on_data_available(&self, _: &DataReader<DynamicData>) {
        if let Some(f) = self.0.on_data_available {
            let _ = std::panic::catch_unwind(|| unsafe { f(self.0.self_) });
        }
    }
    fn on_subscription_matched(&self, _: &DataReader<DynamicData>, s: MatchedStatus) {
        call!(self.0, on_subscription_matched, RddsMatchedStatus::from(s));
    }
    fn on_liveliness_changed(&self, _: &DataReader<DynamicData>, s: LivelinessChangedStatus) {
        call!(
            self.0,
            on_liveliness_changed,
            RddsLivelinessChangedStatus::from(s)
        );
    }
    fn on_requested_deadline_missed(&self, _: &DataReader<DynamicData>, s: DeadlineMissedStatus) {
        call!(
            self.0,
            on_requested_deadline_missed,
            RddsDeadlineMissedStatus::from(s)
        );
    }
    fn on_requested_incompatible_qos(&self, _: &DataReader<DynamicData>, s: IncompatibleQosStatus) {
        call!(
            self.0,
            on_requested_incompatible_qos,
            RddsIncompatibleQosStatus::from(s)
        );
    }
    fn on_sample_rejected(&self, _: &DataReader<DynamicData>, s: SampleRejectedStatus) {
        call!(
            self.0,
            on_sample_rejected,
            RddsSampleRejectedStatus::from(s)
        );
    }
    fn on_sample_lost(&self, _: &DataReader<DynamicData>, s: SampleLostStatus) {
        call!(self.0, on_sample_lost, RddsCountStatus::from(s));
    }
}

// ---------- entities ------------------------------------------------------

/// `rdds_topic`.
#[derive(Debug)]
pub struct RddsTopic {
    topic: Topic<DynamicData>,
    ty: DynamicType,
}

/// `rdds_writer`.
#[derive(Debug)]
pub struct RddsWriter {
    pub(crate) inner: DataWriter<DynamicData>,
    ty: DynamicType,
}

/// `rdds_reader`.
#[derive(Debug)]
pub struct RddsReader {
    pub(crate) inner: DataReader<DynamicData>,
    ty: DynamicType,
}

/// `RDDS_PARTICIPANT_LOCALHOST`: only the loopback interface (tests).
pub const PARTICIPANT_LOCALHOST: u32 = 1;

/// `rdds_participant_new`: joins `domain_id`; `qos` NULL for the default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_new(
    domain_id: u32,
    qos: *const RddsParticipantQos,
    flags: u32,
    out: *mut *mut DomainParticipant,
) -> Status {
    guard(|| {
        let mut config = if flags & PARTICIPANT_LOCALHOST != 0 {
            ParticipantConfig::localhost(domain_id)
        } else {
            ParticipantConfig::new(domain_id)
        };
        if let Some(q) = unsafe { qos.as_ref() } {
            config.user_data = unsafe { slice(q.user_data, q.user_data_len) }?.to_vec();
        }
        let p = DomainParticipant::with_config(config)?;
        unsafe { put(out, p) }
    })
}

/// `rdds_participant_release`: deletes the participant (its entities
/// stop working).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_release(p: *mut DomainParticipant) {
    unsafe { release(p) }
}

/// `rdds_participant_domain_id`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_domain_id(p: *const DomainParticipant) -> u32 {
    guard_value(0, || unsafe { p.as_ref() }.map_or(0, |p| p.domain_id()))
}

/// `rdds_participant_guid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_guid(
    p: *const DomainParticipant,
    out: *mut RddsGuid,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        unsafe { set(out, RddsGuid::of(Some(p.guid()))) }
    })
}

/// `rdds_participant_assert_liveliness`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_assert_liveliness(p: *const DomainParticipant) -> Status {
    guard(|| Ok(unsafe { obj(p, "the participant") }?.assert_liveliness()?))
}

/// `rdds_participant_discovered_participants`: how many other
/// participants are known.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_discovered_participants(
    p: *const DomainParticipant,
) -> usize {
    guard_value(0, || {
        unsafe { p.as_ref() }.map_or(0, |p| p.discovered_participants().len())
    })
}

/// `rdds_participant_create_topic`: a topic of `type`, registered under
/// its name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_create_topic(
    p: *const DomainParticipant,
    name: *const c_char,
    ty: *const DynamicType,
    out: *mut *mut RddsTopic,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let name = unsafe { cstr(name, "the topic name") }?;
        let ty = unsafe { obj(ty, "the type") }?.clone();
        let topic = p.create_dynamic_topic(name, &ty)?;
        unsafe { put(out, RddsTopic { topic, ty }) }
    })
}

/// `rdds_topic_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_topic_release(t: *mut RddsTopic) {
    unsafe { release(t) }
}

/// `rdds_topic_inconsistent_topic_status`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_topic_inconsistent_topic_status(
    t: *const RddsTopic,
    out: *mut RddsCountStatus,
) -> Status {
    guard(|| {
        let t = unsafe { obj(t, "the topic") }?;
        unsafe { set(out, t.topic.inconsistent_topic_status().into()) }
    })
}

/// `rdds_cftopic`: a content filtered topic, and its type.
#[derive(Debug)]
pub struct RddsCfTopic {
    topic: ContentFilteredTopic<DynamicData>,
    ty: DynamicType,
}

/// `n` C strings at `v`.
unsafe fn strings(v: *const *const c_char, n: usize, what: &str) -> Res<Vec<String>> {
    unsafe { slice(v, n) }?
        .iter()
        .map(|p| Ok(unsafe { cstr(*p, what) }?.to_owned()))
        .collect()
}

/// `rdds_participant_create_contentfilteredtopic`: a content filtered
/// topic named `name` of `related`, with the filter `expression`
/// (DDS 1.4, Annex B) and `n` parameters at `parameters`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_create_contentfilteredtopic(
    p: *const DomainParticipant,
    name: *const c_char,
    related: *const RddsTopic,
    expression: *const c_char,
    parameters: *const *const c_char,
    n: usize,
    out: *mut *mut RddsCfTopic,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let name = unsafe { cstr(name, "the topic name") }?;
        let related = unsafe { obj(related, "the related topic") }?;
        let expression = unsafe { cstr(expression, "the filter expression") }?;
        let parameters = unsafe { strings(parameters, n, "an expression parameter") }?;
        let topic = p.create_contentfilteredtopic(name, &related.topic, expression, &parameters)?;
        unsafe {
            put(
                out,
                RddsCfTopic {
                    topic,
                    ty: related.ty.clone(),
                },
            )
        }
    })
}

/// `rdds_cftopic_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_cftopic_release(t: *mut RddsCfTopic) {
    unsafe { release(t) }
}

/// `rdds_cftopic_set_expression_parameters`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_cftopic_set_expression_parameters(
    t: *const RddsCfTopic,
    parameters: *const *const c_char,
    n: usize,
) -> Status {
    guard(|| {
        let t = unsafe { obj(t, "the topic") }?;
        let parameters = unsafe { strings(parameters, n, "an expression parameter") }?;
        t.topic.set_expression_parameters(&parameters)?;
        Ok(())
    })
}

/// `rdds_participant_create_publisher`: `qos` NULL for the default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_create_publisher(
    p: *const DomainParticipant,
    qos: *const RddsGroupQos,
    out: *mut *mut Publisher,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let qos = PublisherQos {
            partition: unsafe { partition(qos) }?,
        };
        unsafe { put(out, p.create_publisher_with_qos(qos)?) }
    })
}

/// `rdds_publisher_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_publisher_release(p: *mut Publisher) {
    unsafe { release(p) }
}

/// `rdds_participant_create_subscriber`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_create_subscriber(
    p: *const DomainParticipant,
    qos: *const RddsGroupQos,
    out: *mut *mut Subscriber,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let qos = SubscriberQos {
            partition: unsafe { partition(qos) }?,
        };
        unsafe { put(out, p.create_subscriber_with_qos(qos)?) }
    })
}

/// `rdds_subscriber_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_release(s: *mut Subscriber) {
    unsafe { release(s) }
}

// ---------- writer --------------------------------------------------------

/// `rdds_publisher_create_writer`: `qos` NULL for the default; the
/// `listener`, if not NULL, is set before the writer is matched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_publisher_create_writer(
    p: *const Publisher,
    topic: *const RddsTopic,
    qos: *const RddsWriterQos,
    listener: *const RddsWriterListener,
    mask: u32,
    out: *mut *mut RddsWriter,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the publisher") }?;
        let t = unsafe { obj(topic, "the topic") }?;
        let qos = unsafe { writer_qos(qos) }?;
        let inner = match unsafe { listener.as_ref() } {
            None => p.create_datawriter_with_qos(&t.topic, qos)?,
            Some(l) => p.create_datawriter_with_listener(
                &t.topic,
                qos,
                WriterCallbacks(*l),
                StatusMask(mask),
            )?,
        };
        unsafe {
            put(
                out,
                RddsWriter {
                    inner,
                    ty: t.ty.clone(),
                },
            )
        }
    })
}

/// `rdds_writer_release`: deletes the writer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_release(w: *mut RddsWriter) {
    unsafe { release(w) }
}

unsafe fn writer_sample<'a>(
    w: *const RddsWriter,
    sample: *const Out,
) -> Res<(&'a RddsWriter, DynamicData)> {
    let w = unsafe { obj(w, "the writer") }?;
    let s = unsafe { obj(sample, "the sample") }?;
    let d = to_data(&w.ty, s)?;
    Ok((w, d))
}

/// `rdds_writer_guid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_guid(w: *const RddsWriter, out: *mut RddsGuid) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        unsafe { set(out, RddsGuid::of(Some(w.inner.guid()))) }
    })
}

/// `rdds_writer_write`: `timestamp` NULL for now.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_write(
    w: *const RddsWriter,
    sample: *const Out,
    timestamp_: *const RddsTime,
) -> Status {
    guard(|| {
        let (w, d) = unsafe { writer_sample(w, sample) }?;
        let t = unsafe { timestamp(timestamp_) }?;
        Ok(w.inner.write_w_timestamp(&d, t)?)
    })
}

/// `rdds_writer_register_instance`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_register_instance(
    w: *const RddsWriter,
    sample: *const Out,
    out: *mut RddsInstanceHandle,
) -> Status {
    guard(|| {
        let (w, d) = unsafe { writer_sample(w, sample) }?;
        let h = w.inner.register_instance(&d)?;
        unsafe { set(out, RddsInstanceHandle::of(Some(h))) }
    })
}

/// `rdds_writer_unregister_instance`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_unregister_instance(
    w: *const RddsWriter,
    sample: *const Out,
    timestamp_: *const RddsTime,
) -> Status {
    guard(|| {
        let (w, d) = unsafe { writer_sample(w, sample) }?;
        let t = unsafe { timestamp(timestamp_) }?;
        Ok(w.inner.unregister_instance_w_timestamp(&d, t)?)
    })
}

/// `rdds_writer_dispose`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_dispose(
    w: *const RddsWriter,
    sample: *const Out,
    timestamp_: *const RddsTime,
) -> Status {
    guard(|| {
        let (w, d) = unsafe { writer_sample(w, sample) }?;
        let t = unsafe { timestamp(timestamp_) }?;
        Ok(w.inner.dispose_w_timestamp(&d, t)?)
    })
}

/// `rdds_writer_lookup_instance`: `HANDLE_NIL` when it is not known.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_lookup_instance(
    w: *const RddsWriter,
    sample: *const Out,
    out: *mut RddsInstanceHandle,
) -> Status {
    guard(|| {
        let (w, d) = unsafe { writer_sample(w, sample) }?;
        unsafe { set(out, RddsInstanceHandle::of(w.inner.lookup_instance(&d))) }
    })
}

/// `rdds_writer_wait_for_acknowledgments`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_wait_for_acknowledgments(
    w: *const RddsWriter,
    max_wait: RddsDuration,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        Ok(w.inner.wait_for_acknowledgments(max_wait.finite()?)?)
    })
}

/// `rdds_writer_wait_for_subscriptions`: until `count` readers match,
/// or `RETCODE_TIMEOUT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_wait_for_subscriptions(
    w: *const RddsWriter,
    count: usize,
    timeout: RddsDuration,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        if w.inner.wait_for_subscriptions(count, timeout.finite()?) {
            Ok(())
        } else {
            Err(crate::Fail(TIMEOUT, "no subscriptions in time".into()))
        }
    })
}

/// Copies `v` into the caller's array of `cap`, the count into `*len`.
unsafe fn guids(v: Vec<Guid>, buf: *mut RddsGuid, cap: usize, len: *mut usize) -> Res {
    for (i, g) in v.iter().take(cap).enumerate() {
        // SAFETY: room for `cap` (the header).
        unsafe { buf.add(i).write(RddsGuid::of(Some(*g))) };
    }
    unsafe { set(len, v.len()) }
}

/// `rdds_writer_matched_subscriptions`: up to `cap` into `buf`, and how
/// many there are into `*len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_matched_subscriptions(
    w: *const RddsWriter,
    buf: *mut RddsGuid,
    cap: usize,
    len: *mut usize,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        unsafe { guids(w.inner.matched_subscriptions(), buf, cap, len) }
    })
}

/// `rdds_writer_assert_liveliness`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_assert_liveliness(w: *const RddsWriter) -> Status {
    guard(|| Ok(unsafe { obj(w, "the writer") }?.inner.assert_liveliness()?))
}

macro_rules! status_getter {
    ($($name:ident, $handle:ty, $what:literal, $method:ident, $out:ty;)*) => {$(
        #[doc = concat!("`", stringify!($name), "`: reads and resets the change counts.")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(e: *const $handle, out: *mut $out) -> Status {
            guard(|| {
                let e = unsafe { obj(e, $what) }?;
                unsafe { set(out, <$out>::from(e.inner.$method())) }
            })
        }
    )*};
}

status_getter! {
    rdds_writer_publication_matched_status, RddsWriter, "the writer", publication_matched_status, RddsMatchedStatus;
    rdds_writer_offered_incompatible_qos_status, RddsWriter, "the writer", offered_incompatible_qos_status, RddsIncompatibleQosStatus;
    rdds_writer_liveliness_lost_status, RddsWriter, "the writer", liveliness_lost_status, RddsCountStatus;
    rdds_writer_offered_deadline_missed_status, RddsWriter, "the writer", offered_deadline_missed_status, RddsDeadlineMissedStatus;
    rdds_reader_subscription_matched_status, RddsReader, "the reader", subscription_matched_status, RddsMatchedStatus;
    rdds_reader_requested_incompatible_qos_status, RddsReader, "the reader", requested_incompatible_qos_status, RddsIncompatibleQosStatus;
    rdds_reader_liveliness_changed_status, RddsReader, "the reader", liveliness_changed_status, RddsLivelinessChangedStatus;
    rdds_reader_requested_deadline_missed_status, RddsReader, "the reader", requested_deadline_missed_status, RddsDeadlineMissedStatus;
    rdds_reader_sample_lost_status, RddsReader, "the reader", sample_lost_status, RddsCountStatus;
    rdds_reader_sample_rejected_status, RddsReader, "the reader", sample_rejected_status, RddsSampleRejectedStatus;
}

/// `rdds_writer_status_changes`: the mask of the statuses that changed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_status_changes(w: *const RddsWriter) -> u32 {
    guard_value(0, || {
        unsafe { w.as_ref() }.map_or(0, |w| w.inner.status_changes().0)
    })
}

/// `rdds_writer_status_condition`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_status_condition(
    w: *const RddsWriter,
    out: *mut *mut RddsCondition,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        unsafe { put(out, RddsCondition::from(w.inner.status_condition())) }
    })
}

/// `rdds_writer_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_writer_set_listener(
    w: *const RddsWriter,
    listener: *const RddsWriterListener,
    mask: u32,
) -> Status {
    guard(|| {
        let w = unsafe { obj(w, "the writer") }?;
        match unsafe { listener.as_ref() } {
            None => w.inner.clear_listener(),
            Some(l) => w.inner.set_listener(WriterCallbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}

// ---------- reader --------------------------------------------------------

/// `rdds_subscriber_create_reader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_create_reader(
    s: *const Subscriber,
    topic: *const RddsTopic,
    qos: *const RddsReaderQos,
    listener: *const RddsReaderListener,
    mask: u32,
    out: *mut *mut RddsReader,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the subscriber") }?;
        let t = unsafe { obj(topic, "the topic") }?;
        let qos = unsafe { reader_qos(qos) }?;
        let inner = match unsafe { listener.as_ref() } {
            None => s.create_datareader_with_qos(&t.topic, qos)?,
            Some(l) => s.create_datareader_with_listener(
                &t.topic,
                qos,
                ReaderCallbacks(*l),
                StatusMask(mask),
            )?,
        };
        unsafe {
            put(
                out,
                RddsReader {
                    inner,
                    ty: t.ty.clone(),
                },
            )
        }
    })
}

/// `rdds_subscriber_create_filtered_reader`: a reader of a content
/// filtered topic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_create_filtered_reader(
    s: *const Subscriber,
    topic: *const RddsCfTopic,
    qos: *const RddsReaderQos,
    listener: *const RddsReaderListener,
    mask: u32,
    out: *mut *mut RddsReader,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the subscriber") }?;
        let t = unsafe { obj(topic, "the topic") }?;
        let qos = unsafe { reader_qos(qos) }?;
        let inner = match unsafe { listener.as_ref() } {
            None => s.create_datareader_with_qos(&t.topic, qos)?,
            Some(l) => s.create_datareader_with_listener(
                &t.topic,
                qos,
                ReaderCallbacks(*l),
                StatusMask(mask),
            )?,
        };
        unsafe {
            put(
                out,
                RddsReader {
                    inner,
                    ty: t.ty.clone(),
                },
            )
        }
    })
}

/// `rdds_reader_release`: deletes the reader.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_release(r: *mut RddsReader) {
    unsafe { release(r) }
}

/// `rdds_reader_guid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_guid(r: *const RddsReader, out: *mut RddsGuid) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        unsafe { set(out, RddsGuid::of(Some(r.inner.guid()))) }
    })
}

/// `rdds_sample_info`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsSampleInfo {
    /// `READ_SAMPLE_STATE` (1) or `NOT_READ_SAMPLE_STATE` (2).
    pub sample_state: u32,
    /// `NEW_VIEW_STATE` (1) or `NOT_NEW_VIEW_STATE` (2).
    pub view_state: u32,
    /// `ALIVE_INSTANCE_STATE` (1), `NOT_ALIVE_DISPOSED_INSTANCE_STATE`
    /// (2) or `NOT_ALIVE_NO_WRITERS_INSTANCE_STATE` (4).
    pub instance_state: u32,
    pub disposed_generation_count: i32,
    pub no_writers_generation_count: i32,
    pub sample_rank: i32,
    pub generation_rank: i32,
    pub absolute_generation_rank: i32,
    pub source_timestamp: RddsTime,
    pub instance_handle: RddsInstanceHandle,
    pub publication_handle: RddsGuid,
    pub sequence_number: i64,
    pub valid_data: bool,
}

/// `rdds_samples`: what a read or take returned.
#[derive(Debug)]
pub struct RddsSamples(Vec<Sample<DynamicData>>);

/// `RDDS_SELECT_ANY`, `RDDS_SELECT_INSTANCE`, `RDDS_SELECT_NEXT_INSTANCE`.
pub const SELECT_ANY: i32 = 0;
/// See [`SELECT_ANY`].
pub const SELECT_INSTANCE: i32 = 1;
/// See [`SELECT_ANY`].
pub const SELECT_NEXT_INSTANCE: i32 = 2;

fn filter(sample_states: u32, view_states: u32, instance_states: u32) -> Res<StateFilter> {
    let mut f = StateFilter::ANY;
    let mut s = None;
    for (bit, m) in [(1, SampleStateMask::READ), (2, SampleStateMask::NOT_READ)] {
        if sample_states & bit != 0 {
            s = Some(s.map_or(m, |x| x | m));
        }
    }
    f.sample_states = s.ok_or_else(|| bad("no sample state selected"))?;
    let mut v = None;
    for (bit, m) in [(1, ViewStateMask::NEW), (2, ViewStateMask::NOT_NEW)] {
        if view_states & bit != 0 {
            v = Some(v.map_or(m, |x| x | m));
        }
    }
    f.view_states = v.ok_or_else(|| bad("no view state selected"))?;
    let mut i = None;
    for (bit, m) in [
        (1, InstanceStateMask::ALIVE),
        (2, InstanceStateMask::NOT_ALIVE_DISPOSED),
        (4, InstanceStateMask::NOT_ALIVE_NO_WRITERS),
    ] {
        if instance_states & bit != 0 {
            i = Some(i.map_or(m, |x| x | m));
        }
    }
    f.instance_states = i.ok_or_else(|| bad("no instance state selected"))?;
    Ok(f)
}

/// `rdds_reader_select`: reads (or takes, with `take`) the samples in
/// the states of the masks (`RDDS_ANY_*_STATE` for any); with
/// `RDDS_SELECT_INSTANCE`, those of `*instance` only (the masks then
/// ignored); with `RDDS_SELECT_NEXT_INSTANCE`, those of the instance
/// after `*instance` (`HANDLE_NIL` or NULL for the first). An empty
/// result is `RETCODE_OK` with no samples.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_select(
    r: *const RddsReader,
    take: bool,
    sample_states: u32,
    view_states: u32,
    instance_states: u32,
    mode: i32,
    instance: *const RddsInstanceHandle,
    out: *mut *mut RddsSamples,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        let handle = unsafe { instance.as_ref() }
            .map(|h| InstanceHandle(h.value))
            .filter(|h| h.0 != [0; 16]);
        let v = match mode {
            SELECT_ANY => {
                let f = filter(sample_states, view_states, instance_states)?;
                if take {
                    r.inner.take_with(f)
                } else {
                    r.inner.read_with(f)
                }
            }
            SELECT_INSTANCE => {
                let h = handle.ok_or_else(|| bad("no instance given"))?;
                if take {
                    r.inner.take_instance(h)
                } else {
                    r.inner.read_instance(h)
                }
            }
            SELECT_NEXT_INSTANCE => {
                let f = filter(sample_states, view_states, instance_states)?;
                if take {
                    r.inner.take_next_instance(handle, f)
                } else {
                    r.inner.read_next_instance(handle, f)
                }
            }
            m => return Err(bad(format!("unknown selection {m}"))),
        };
        unsafe { put(out, RddsSamples(v)) }
    })
}

/// `rdds_reader_select_w_condition`: the samples a read condition of
/// this reader selects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_select_w_condition(
    r: *const RddsReader,
    take: bool,
    condition: *const RddsCondition,
    out: *mut *mut RddsSamples,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        let c = unsafe { obj(condition, "the condition") }?;
        let rustorb_dds::Condition::Read(c) = &c.0 else {
            return Err(bad("not a read condition"));
        };
        let v = if take {
            r.inner.take_w_condition(c)?
        } else {
            r.inner.read_w_condition(c)?
        };
        unsafe { put(out, RddsSamples(v)) }
    })
}

/// `rdds_samples_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_samples_count(s: *const RddsSamples) -> usize {
    unsafe { s.as_ref() }.map_or(0, |s| s.0.len())
}

/// `rdds_samples_info`: the information of sample `i`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_samples_info(
    s: *const RddsSamples,
    i: usize,
    out: *mut RddsSampleInfo,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the samples") }?;
        let x = s.0.get(i).ok_or_else(|| bad("no such sample"))?;
        let info = &x.info;
        let v = RddsSampleInfo {
            sample_state: match info.sample_state {
                SampleState::Read => 1,
                SampleState::NotRead => 2,
            },
            view_state: match info.view_state {
                ViewState::New => 1,
                ViewState::NotNew => 2,
            },
            instance_state: match info.instance_state {
                InstanceState::Alive => 1,
                InstanceState::NotAliveDisposed => 2,
                InstanceState::NotAliveNoWriters => 4,
            },
            disposed_generation_count: info.disposed_generation_count as i32,
            no_writers_generation_count: info.no_writers_generation_count as i32,
            sample_rank: info.sample_rank as i32,
            generation_rank: info.generation_rank as i32,
            absolute_generation_rank: info.absolute_generation_rank as i32,
            source_timestamp: RddsTime::of(info.source_timestamp),
            instance_handle: RddsInstanceHandle::of(Some(info.instance_handle)),
            publication_handle: RddsGuid::of(Some(info.writer)),
            sequence_number: info.sequence_number.0,
            valid_data: info.valid_data,
        };
        unsafe { set(out, v) }
    })
}

/// `rdds_samples_data`: an input stream of sample `i`'s values, released
/// by the caller; `RETCODE_NO_DATA` for a sample without data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_samples_data(
    s: *const RddsSamples,
    i: usize,
    out: *mut *mut In,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the samples") }?;
        let x = s.0.get(i).ok_or_else(|| bad("no such sample"))?;
        let d = x
            .data
            .as_ref()
            .ok_or_else(|| crate::Fail(crate::NO_DATA, "the sample has no data".into()))?;
        unsafe { put(out, from_data(d)?) }
    })
}

/// `rdds_samples_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_samples_release(s: *mut RddsSamples) {
    unsafe { release(s) }
}

/// `rdds_reader_lookup_instance`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_lookup_instance(
    r: *const RddsReader,
    sample: *const Out,
    out: *mut RddsInstanceHandle,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        let s = unsafe { obj(sample, "the sample") }?;
        let d = to_data(&r.ty, s)?;
        unsafe { set(out, RddsInstanceHandle::of(r.inner.lookup_instance(&d))) }
    })
}

/// `rdds_reader_get_key_value`: a sample with the key of an instance
/// (the other members default), or `RETCODE_BAD_PARAMETER` for an
/// unknown instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_get_key_value(
    r: *const RddsReader,
    instance: RddsInstanceHandle,
    out: *mut *mut In,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        let d = r
            .inner
            .get_key_value(InstanceHandle(instance.value))
            .ok_or_else(|| bad("unknown instance"))?;
        unsafe { put(out, from_data(&d)?) }
    })
}

/// `rdds_reader_wait_for_data`: until a sample is there, or
/// `RETCODE_TIMEOUT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_wait_for_data(
    r: *const RddsReader,
    timeout: RddsDuration,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        if r.inner.wait_for_data(timeout.finite()?) {
            Ok(())
        } else {
            Err(crate::Fail(TIMEOUT, "no data in time".into()))
        }
    })
}

/// `rdds_reader_wait_for_publications`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_wait_for_publications(
    r: *const RddsReader,
    count: usize,
    timeout: RddsDuration,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        if r.inner.wait_for_publications(count, timeout.finite()?) {
            Ok(())
        } else {
            Err(crate::Fail(TIMEOUT, "no publications in time".into()))
        }
    })
}

/// `rdds_reader_matched_publications`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_matched_publications(
    r: *const RddsReader,
    buf: *mut RddsGuid,
    cap: usize,
    len: *mut usize,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        unsafe { guids(r.inner.matched_publications(), buf, cap, len) }
    })
}

/// `rdds_reader_status_changes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_status_changes(r: *const RddsReader) -> u32 {
    guard_value(0, || {
        unsafe { r.as_ref() }.map_or(0, |r| r.inner.status_changes().0)
    })
}

/// `rdds_reader_status_condition`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_status_condition(
    r: *const RddsReader,
    out: *mut *mut RddsCondition,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        unsafe { put(out, RddsCondition::from(r.inner.status_condition())) }
    })
}

/// `rdds_reader_create_readcondition`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_create_readcondition(
    r: *const RddsReader,
    sample_states: u32,
    view_states: u32,
    instance_states: u32,
    out: *mut *mut RddsCondition,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        let f = filter(sample_states, view_states, instance_states)?;
        unsafe { put(out, RddsCondition::from(r.inner.create_readcondition(f))) }
    })
}

/// `rdds_reader_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_reader_set_listener(
    r: *const RddsReader,
    listener: *const RddsReaderListener,
    mask: u32,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the reader") }?;
        match unsafe { listener.as_ref() } {
            None => r.inner.clear_listener(),
            Some(l) => r.inner.set_listener(ReaderCallbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}
