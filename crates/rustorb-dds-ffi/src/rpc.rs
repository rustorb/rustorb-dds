//! Requests and replies (DDS-RPC 1.0): `rdds_requester` and
//! `rdds_replier` over the [`rpc`](rustorb_dds::rpc) module, with samples
//! of the topics' types.

use std::collections::VecDeque;
use std::ffi::c_char;
use std::sync::Mutex;

use rustorb_dds::rpc::{Replier, Request, Requester, RpcConfig};
use rustorb_dds::{
    DomainParticipant, DynamicData, DynamicType, Guid, SampleIdentity, SequenceNumber,
};

use crate::entity::{
    RddsDuration, RddsGuid, RddsReaderQos, RddsTopic, RddsWriterQos, reader_qos, writer_qos,
};
use crate::stream::{In, Out, from_data, to_data};
use crate::{NO_DATA, Res, Status, TIMEOUT, bad, cstr, guard, obj, put, release, set};

/// `rdds_sample_identity`: a writer and a sequence number.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RddsSampleIdentity {
    pub writer: RddsGuid,
    pub sequence_number: i64,
}

impl RddsSampleIdentity {
    pub(crate) fn of(id: SampleIdentity) -> Self {
        RddsSampleIdentity {
            writer: RddsGuid::of(Some(id.writer_guid)),
            sequence_number: id.sequence_number.0,
        }
    }

    fn get(&self) -> SampleIdentity {
        SampleIdentity {
            writer_guid: Guid::from_bytes(self.writer.value),
            sequence_number: SequenceNumber(self.sequence_number),
        }
    }
}

/// The QoS and topics of a requester or replier.
unsafe fn config(
    wq: *const RddsWriterQos,
    rq: *const RddsReaderQos,
    instance_name: *const c_char,
) -> Res<RpcConfig> {
    let mut c = RpcConfig::with_topics("", "");
    if !wq.is_null() {
        c.writer_qos = unsafe { writer_qos(wq) }?;
    }
    if !rq.is_null() {
        c.reader_qos = unsafe { reader_qos(rq) }?;
    }
    if !instance_name.is_null() {
        c.service_instance_name = Some(unsafe { cstr(instance_name, "the instance name") }?.into());
    }
    Ok(c)
}

/// `rdds_requester`.
#[derive(Debug)]
pub struct RddsRequester {
    inner: Requester<DynamicData, DynamicData>,
    request_type: DynamicType,
}

/// `rdds_requester_new`: a requester on `request` and `reply`; the QoS
/// NULL for the default (RELIABLE, KEEP_ALL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_requester_new(
    p: *const DomainParticipant,
    request: *const RddsTopic,
    reply: *const RddsTopic,
    writer_qos: *const RddsWriterQos,
    reader_qos: *const RddsReaderQos,
    out: *mut *mut RddsRequester,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let request = unsafe { obj(request, "the request topic") }?;
        let reply = unsafe { obj(reply, "the reply topic") }?;
        let c = unsafe { config(writer_qos, reader_qos, std::ptr::null()) }?;
        let inner = Requester::with_topics(p, &request.topic, &reply.topic, &c)?;
        unsafe {
            put(
                out,
                RddsRequester {
                    inner,
                    request_type: request.ty.clone(),
                },
            )
        }
    })
}

/// `rdds_requester_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_requester_release(r: *mut RddsRequester) {
    unsafe { release(r) }
}

/// `rdds_requester_wait_for_service`: until a replier is matched, or
/// `RETCODE_TIMEOUT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_requester_wait_for_service(
    r: *const RddsRequester,
    timeout: RddsDuration,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the requester") }?;
        if r.inner.wait_for_service(timeout.finite()?) {
            Ok(())
        } else {
            Err(crate::Fail(TIMEOUT, "no service in time".into()))
        }
    })
}

/// `rdds_requester_send_request`: the request's identity into `*out`
/// (NULL to ignore it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_requester_send_request(
    r: *const RddsRequester,
    sample: *const Out,
    out: *mut RddsSampleIdentity,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the requester") }?;
        let s = unsafe { obj(sample, "the sample") }?;
        let id = r.inner.send_request(&to_data(&r.request_type, s)?)?;
        if !out.is_null() {
            unsafe { set(out, RddsSampleIdentity::of(id)) }?;
        }
        Ok(())
    })
}

/// `rdds_requester_receive_reply`: waits for a reply to `request`, or
/// `RETCODE_TIMEOUT`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_requester_receive_reply(
    r: *const RddsRequester,
    request: *const RddsSampleIdentity,
    timeout: RddsDuration,
    out: *mut *mut In,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the requester") }?;
        let id = unsafe { obj(request, "the request") }?.get();
        let d = r.inner.receive_reply(&id, timeout.finite()?)?;
        unsafe { put(out, from_data(&d)?) }
    })
}

/// `rdds_replier`.
#[derive(Debug)]
pub struct RddsReplier {
    inner: Replier<DynamicData, DynamicData>,
    reply_type: DynamicType,
    /// Requests taken and not yet handed out.
    pending: Mutex<VecDeque<Request<DynamicData>>>,
}

/// `rdds_replier_new`: a replier on `request` and `reply`; the QoS NULL
/// for the default (RELIABLE, KEEP_ALL), `instance_name` NULL for none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_replier_new(
    p: *const DomainParticipant,
    request: *const RddsTopic,
    reply: *const RddsTopic,
    writer_qos: *const RddsWriterQos,
    reader_qos: *const RddsReaderQos,
    instance_name: *const c_char,
    out: *mut *mut RddsReplier,
) -> Status {
    guard(|| {
        let p = unsafe { obj(p, "the participant") }?;
        let request = unsafe { obj(request, "the request topic") }?;
        let reply = unsafe { obj(reply, "the reply topic") }?;
        let c = unsafe { config(writer_qos, reader_qos, instance_name) }?;
        let inner = Replier::with_topics(p, &request.topic, &reply.topic, &c)?;
        unsafe {
            put(
                out,
                RddsReplier {
                    inner,
                    reply_type: reply.ty.clone(),
                    pending: Mutex::new(VecDeque::new()),
                },
            )
        }
    })
}

/// `rdds_replier_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_replier_release(r: *mut RddsReplier) {
    unsafe { release(r) }
}

/// `rdds_replier_receive_request`: waits for a request, or
/// `RETCODE_TIMEOUT`; the request into `*out`, what its reply names into
/// `*id`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_replier_receive_request(
    r: *const RddsReplier,
    timeout: RddsDuration,
    out: *mut *mut In,
    id: *mut RddsSampleIdentity,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the replier") }?;
        if out.is_null() || id.is_null() {
            return Err(bad("the output pointer is NULL"));
        }
        let mut pending = r
            .pending
            .lock()
            .map_err(|_| bad("the replier is poisoned"))?;
        if pending.is_empty() {
            pending.extend(r.inner.receive_requests(timeout.finite()?)?);
        }
        let req = pending
            .pop_front()
            .ok_or_else(|| crate::Fail(TIMEOUT, "no request in time".into()))?;
        unsafe { set(id, RddsSampleIdentity::of(req.id)) }?;
        unsafe { put(out, from_data(&req.data)?) }
    })
}

/// `rdds_replier_send_reply`: `sample` to the request `request`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_replier_send_reply(
    r: *const RddsReplier,
    sample: *const Out,
    request: *const RddsSampleIdentity,
) -> Status {
    guard(|| {
        let r = unsafe { obj(r, "the replier") }?;
        let s = unsafe { obj(sample, "the sample") }?;
        let id = unsafe { obj(request, "the request") }?.get();
        Ok(r.inner.send_reply(&to_data(&r.reply_type, s)?, &id)?)
    })
}

/// `rdds_samples_related_sample_identity`: the related sample identity of
/// sample `i` (what a reply names), or `RETCODE_NO_DATA` if it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_samples_related_sample_identity(
    s: *const crate::entity::RddsSamples,
    i: usize,
    out: *mut RddsSampleIdentity,
) -> Status {
    guard(|| {
        let s = unsafe { obj(s, "the samples") }?;
        let id = s
            .sample(i)?
            .info
            .related_sample_identity
            .ok_or_else(|| crate::Fail(NO_DATA, "no related sample identity".into()))?;
        unsafe { set(out, RddsSampleIdentity::of(id)) }
    })
}
