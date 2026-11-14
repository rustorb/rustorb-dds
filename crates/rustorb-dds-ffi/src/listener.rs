//! The listeners of participants, publishers, subscribers and topics
//! (`rdds_listener`), which take the statuses of their writers and
//! readers that the writers' and readers' own listeners do not (DDS 1.4,
//! 2.2.4.3).

use std::ffi::{CString, c_char, c_void};

use rustorb_dds::{
    AnyDataReader, AnyDataWriter, DeadlineMissedStatus, DomainParticipant,
    DomainParticipantListener, IncompatibleQosStatus, InconsistentTopicStatus,
    LivelinessChangedStatus, LivelinessLostStatus, MatchedStatus, Publisher, PublisherListener,
    SampleLostStatus, SampleRejectedStatus, StatusMask, Subscriber, SubscriberListener,
    TopicListener,
};

use crate::entity::{
    RddsCountStatus, RddsDeadlineMissedStatus, RddsGuid, RddsIncompatibleQosStatus,
    RddsLivelinessChangedStatus, RddsMatchedStatus, RddsSampleRejectedStatus, RddsTopic,
};
use crate::{Status, guard, obj, put};

/// `rdds_endpoint_info`: the writer or reader a status is of, valid for
/// the call.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsEndpointInfo {
    pub guid: RddsGuid,
    pub topic_name: *const c_char,
    pub type_name: *const c_char,
}

type WriterFn<S> = Option<unsafe extern "C" fn(*mut c_void, *const RddsEndpointInfo, *const S)>;
type ReaderFn<S> = WriterFn<S>;

/// `rdds_listener`: functions (any may be NULL) called with `self`; those
/// that do not fit the entity it is set on are not called. `release`, if
/// not NULL, is called once when the listener is replaced or cleared or
/// its entity is deleted.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RddsListener {
    pub self_: *mut c_void,
    pub on_offered_deadline_missed: WriterFn<RddsDeadlineMissedStatus>,
    pub on_offered_incompatible_qos: WriterFn<RddsIncompatibleQosStatus>,
    pub on_liveliness_lost: WriterFn<RddsCountStatus>,
    pub on_publication_matched: WriterFn<RddsMatchedStatus>,
    pub on_data_on_readers: Option<unsafe extern "C" fn(*mut c_void, *const Subscriber)>,
    pub on_requested_deadline_missed: ReaderFn<RddsDeadlineMissedStatus>,
    pub on_requested_incompatible_qos: ReaderFn<RddsIncompatibleQosStatus>,
    pub on_sample_rejected: ReaderFn<RddsSampleRejectedStatus>,
    pub on_liveliness_changed: ReaderFn<RddsLivelinessChangedStatus>,
    pub on_data_available: Option<unsafe extern "C" fn(*mut c_void, *const RddsEndpointInfo)>,
    pub on_subscription_matched: ReaderFn<RddsMatchedStatus>,
    pub on_sample_lost: ReaderFn<RddsCountStatus>,
    pub on_inconsistent_topic:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const RddsCountStatus)>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

struct Callbacks(RddsListener);

// SAFETY: the caller's functions may be called from any thread (the
// header says so of listeners).
unsafe impl Send for Callbacks {}
unsafe impl Sync for Callbacks {}

impl Drop for Callbacks {
    fn drop(&mut self) {
        if let Some(f) = self.0.release {
            unsafe { f(self.0.self_) }
        }
    }
}

/// Calls `f` with the information of an endpoint, its names as C
/// strings for the call.
fn with_info(
    guid: rustorb_dds::Guid,
    topic: String,
    ty: String,
    f: impl FnOnce(&RddsEndpointInfo),
) {
    let topic = CString::new(topic).unwrap_or_default();
    let ty = CString::new(ty).unwrap_or_default();
    let info = RddsEndpointInfo {
        guid: RddsGuid::of(Some(guid)),
        topic_name: topic.as_ptr(),
        type_name: ty.as_ptr(),
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&info)));
}

macro_rules! writer_call {
    ($l:expr, $f:ident, $w:expr, $s:expr) => {
        if let Some(f) = $l.$f {
            let s = $s;
            let w: &AnyDataWriter = $w;
            with_info(w.guid(), w.topic_name(), w.type_name(), |i| unsafe {
                f($l.self_, i, &s)
            });
        }
    };
}

macro_rules! reader_call {
    ($l:expr, $f:ident, $r:expr, $s:expr) => {
        if let Some(f) = $l.$f {
            let s = $s;
            let r: &AnyDataReader = $r;
            with_info(r.guid(), r.topic_name(), r.type_name(), |i| unsafe {
                f($l.self_, i, &s)
            });
        }
    };
}

impl PublisherListener for Callbacks {
    fn on_publication_matched(&self, w: &AnyDataWriter, s: MatchedStatus) {
        writer_call!(
            self.0,
            on_publication_matched,
            w,
            RddsMatchedStatus::from(s)
        );
    }
    fn on_liveliness_lost(&self, w: &AnyDataWriter, s: LivelinessLostStatus) {
        writer_call!(self.0, on_liveliness_lost, w, RddsCountStatus::from(s));
    }
    fn on_offered_deadline_missed(&self, w: &AnyDataWriter, s: DeadlineMissedStatus) {
        writer_call!(
            self.0,
            on_offered_deadline_missed,
            w,
            RddsDeadlineMissedStatus::from(s)
        );
    }
    fn on_offered_incompatible_qos(&self, w: &AnyDataWriter, s: IncompatibleQosStatus) {
        writer_call!(
            self.0,
            on_offered_incompatible_qos,
            w,
            RddsIncompatibleQosStatus::from(s)
        );
    }
}

impl SubscriberListener for Callbacks {
    fn on_data_on_readers(&self, subscriber: &Subscriber) {
        if let Some(f) = self.0.on_data_on_readers {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                f(self.0.self_, subscriber)
            }));
        }
    }
    fn on_data_available(&self, r: &AnyDataReader) {
        if let Some(f) = self.0.on_data_available {
            with_info(r.guid(), r.topic_name(), r.type_name(), |i| unsafe {
                f(self.0.self_, i)
            });
        }
    }
    fn on_subscription_matched(&self, r: &AnyDataReader, s: MatchedStatus) {
        reader_call!(
            self.0,
            on_subscription_matched,
            r,
            RddsMatchedStatus::from(s)
        );
    }
    fn on_liveliness_changed(&self, r: &AnyDataReader, s: LivelinessChangedStatus) {
        reader_call!(
            self.0,
            on_liveliness_changed,
            r,
            RddsLivelinessChangedStatus::from(s)
        );
    }
    fn on_requested_deadline_missed(&self, r: &AnyDataReader, s: DeadlineMissedStatus) {
        reader_call!(
            self.0,
            on_requested_deadline_missed,
            r,
            RddsDeadlineMissedStatus::from(s)
        );
    }
    fn on_requested_incompatible_qos(&self, r: &AnyDataReader, s: IncompatibleQosStatus) {
        reader_call!(
            self.0,
            on_requested_incompatible_qos,
            r,
            RddsIncompatibleQosStatus::from(s)
        );
    }
    fn on_sample_rejected(&self, r: &AnyDataReader, s: SampleRejectedStatus) {
        reader_call!(
            self.0,
            on_sample_rejected,
            r,
            RddsSampleRejectedStatus::from(s)
        );
    }
    fn on_sample_lost(&self, r: &AnyDataReader, s: SampleLostStatus) {
        reader_call!(self.0, on_sample_lost, r, RddsCountStatus::from(s));
    }
}

impl TopicListener for Callbacks {
    fn on_inconsistent_topic(&self, topic_name: &str, s: InconsistentTopicStatus) {
        if let Some(f) = self.0.on_inconsistent_topic {
            let name = CString::new(topic_name).unwrap_or_default();
            let s = RddsCountStatus::from(s);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                f(self.0.self_, name.as_ptr(), &s)
            }));
        }
    }
}

impl DomainParticipantListener for Callbacks {}

/// `rdds_participant_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_participant_set_listener(
    p: *const DomainParticipant,
    listener: *const RddsListener,
    mask: u32,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        match unsafe { listener.as_ref() } {
            None => p.clear_listener(),
            Some(l) => p.set_listener(Callbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}

/// `rdds_publisher_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_publisher_set_listener(
    p: *const Publisher,
    listener: *const RddsListener,
    mask: u32,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the publisher") }?;
        match unsafe { listener.as_ref() } {
            None => p.clear_listener(),
            Some(l) => p.set_listener(Callbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}

/// `rdds_subscriber_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_set_listener(
    s: *const Subscriber,
    listener: *const RddsListener,
    mask: u32,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the subscriber") }?;
        match unsafe { listener.as_ref() } {
            None => s.clear_listener(),
            Some(l) => s.set_listener(Callbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}

/// `rdds_topic_set_listener`: NULL clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_topic_set_listener(
    t: *const RddsTopic,
    listener: *const RddsListener,
    mask: u32,
) -> Status {
    guard(|| {
        let t = unsafe { obj(t, "the topic") }?;
        match unsafe { listener.as_ref() } {
            None => t.topic.clear_listener(),
            Some(l) => t.topic.set_listener(Callbacks(*l), StatusMask(mask)),
        }
        Ok(())
    })
}

/// `rdds_subscriber_notify_datareaders`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_notify_datareaders(s: *const Subscriber) -> Status {
    guard(|| {
        unsafe { obj(s, "the subscriber") }?.notify_datareaders();
        Ok(())
    })
}

/// `rdds_subscriber_clone`: another handle of the same subscriber.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_clone(
    s: *const Subscriber,
    out: *mut *mut Subscriber,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the subscriber") }?;
        unsafe { put(out, s.clone()) }
    })
}

/// `rdds_subscriber_same`: whether two handles are of the same
/// subscriber.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_subscriber_same(a: *const Subscriber, b: *const Subscriber) -> bool {
    match unsafe { (a.as_ref(), b.as_ref()) } {
        (Some(a), Some(b)) => a.same_as(b),
        _ => false,
    }
}
