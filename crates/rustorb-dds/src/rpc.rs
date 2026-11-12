//! Requests and replies over DDS (DDS-RPC 1.0): a [`Requester`] sends
//! requests on a request topic and gets the replies to them on a reply
//! topic; a [`Replier`] takes the requests and answers each one.
//!
//! A reply names the request it answers with its related sample identity
//! (`PID_RELATED_SAMPLE_IDENTITY`, sent in-line, DDS-RPC 1.0, 7.8.2): the
//! writer and sequence number of the request, as in the Enhanced Service
//! Mapping (7.4.3), so the request and reply types are the user's own, with
//! no header. The topics are `<service>_Request` and `<service>_Reply`
//! (7.4.1), or names given in [`RpcConfig`].
//!
//! A request also names, as its related sample identity, the requester's
//! reply reader (with `SEQUENCENUMBER_UNKNOWN`), and a replier answers a
//! request that names one with that reader and the request's sequence
//! number: ROS 2's rmw_fastrtps_cpp does both (seen in captures), so its
//! clients and services work with these. Others ignore the identity of a
//! request (7.8.2 gives it to replies).
//!
//! Each side announces its writer and reader as a pair through discovery,
//! with `PID_RELATED_ENTITY_GUID` (7.6.2.1), and holds back what would
//! go astray while the other side's pair is half discovered (7.6.2.2): a
//! [`Replier`] does not hand out the requests of a requester until it has
//! matched that requester's reply reader, and
//! [`Requester::wait_for_service`] waits for a replier both of whose
//! endpoints are matched. A remote endpoint that does not announce a
//! related entity is paired with those of the same participant.
//!
//! ROS 2's rmw_cyclonedds_cpp puts a header in the payload instead,
//! between the encapsulation and the body (seen in captures): the client's
//! 8-octet id and a 64-bit sequence number of its own, in the byte order of
//! the encapsulation, which the service sends back at the head of the
//! reply. With the endpoints of a participant of Cyclone DDS (vendor
//! 0x0110), a [`Requester`] and a [`Replier`] write and read that header.
//! Its services answer only a client whose reply reader they have matched,
//! known by `clientid= ...;` in the USER_DATA of the client's writer and
//! reader, which a [`Requester`] announces.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rustorb_rtps::{Guid, SampleIdentity, SequenceNumber, VendorId};
use rustorb_typesupport::TopicType;

use crate::participant::Shared;
use crate::publisher::{DataWriter, WriteParams};
use crate::qos::{DataReaderQos, DataWriterQos, History, Reliability};
use crate::subscriber::DataReader;
use crate::{DomainParticipant, Error, Result, SampleInfo, Topic};

/// The vendor of Cyclone DDS, whose ROS 2 services carry a header in the
/// payload.
const CYCLONE: VendorId = VendorId([0x01, 0x10]);

/// The length of the encapsulation and of the in-payload header.
const ENCAP: usize = 4;
const HEADER: usize = 16;

/// An in-payload header: the client id and the sequence number.
type Header = ([u8; 8], i64);

/// Whether the endpoint `guid` is of a participant of Cyclone DDS.
fn is_cyclone(shared: &Shared, guid: &Guid) -> bool {
    shared.vendor_of(&guid.prefix) == Some(CYCLONE)
}

/// Whether a payload's encapsulation is little-endian.
fn little_endian(payload: &[u8]) -> bool {
    payload.get(1).is_some_and(|b| b & 1 == 1)
}

/// Puts the header (`client`, `seq`) after the encapsulation of `payload`.
fn insert_header(mut payload: Vec<u8>, client: [u8; 8], seq: i64) -> Vec<u8> {
    let mut header = [0u8; HEADER];
    header[..8].copy_from_slice(&client);
    header[8..].copy_from_slice(&if little_endian(&payload) {
        seq.to_le_bytes()
    } else {
        seq.to_be_bytes()
    });
    let at = ENCAP.min(payload.len());
    payload.splice(at..at, header);
    payload
}

/// Takes the header out of `payload`: the client id and the sequence
/// number, and the payload left.
fn strip_header(payload: &[u8]) -> Option<([u8; 8], i64, Vec<u8>)> {
    if payload.len() < ENCAP + HEADER {
        return None;
    }
    let h = &payload[ENCAP..ENCAP + HEADER];
    let client: [u8; 8] = h[..8].try_into().ok()?;
    let seq: [u8; 8] = h[8..].try_into().ok()?;
    let seq = if little_endian(payload) {
        i64::from_le_bytes(seq)
    } else {
        i64::from_be_bytes(seq)
    };
    let mut rest = payload[..ENCAP].to_vec();
    rest.extend_from_slice(&payload[ENCAP + HEADER..]);
    Some((client, seq, rest))
}

/// Takes the samples of `reader`, deserialized, with the header of those
/// written by a participant of Cyclone DDS taken out of the payload.
fn take_with_headers<T>(reader: &DataReader<T>) -> Vec<(T, SampleInfo, Option<Header>)> {
    let shared = &reader.shared;
    reader
        .take_raw()
        .into_iter()
        .filter_map(|(payload, info)| {
            let payload = payload?;
            if is_cyclone(shared, &info.writer) {
                let (client, seq, rest) = strip_header(&payload)?;
                Some((reader.decode_raw(&rest)?, info, Some((client, seq))))
            } else {
                Some((reader.decode_raw(&payload)?, info, None))
            }
        })
        .collect()
}

/// How a [`Requester`] or [`Replier`] is made.
#[derive(Clone, Debug)]
pub struct RpcConfig {
    /// The topic of the requests.
    pub request_topic: String,
    /// The topic of the replies.
    pub reply_topic: String,
    /// The service instance name a replier announces
    /// (`PID_SERVICE_INSTANCE_NAME`, DDS-RPC 1.0, 7.6.2.1); a requester
    /// announces none.
    pub service_instance_name: Option<String>,
    /// The QoS of the writer (of requests for a requester, of replies for
    /// a replier).
    pub writer_qos: DataWriterQos,
    /// The QoS of the reader.
    pub reader_qos: DataReaderQos,
}

impl RpcConfig {
    /// The topics `<service>_Request` and `<service>_Reply` (DDS-RPC
    /// 1.0, 7.4.1), RELIABLE and KEEP_ALL on both sides.
    pub fn new(service: &str) -> Self {
        RpcConfig::with_topics(&format!("{service}_Request"), &format!("{service}_Reply"))
    }

    /// The topics given, RELIABLE and KEEP_ALL on both sides.
    pub fn with_topics(request_topic: &str, reply_topic: &str) -> Self {
        RpcConfig {
            request_topic: request_topic.to_string(),
            reply_topic: reply_topic.to_string(),
            service_instance_name: None,
            writer_qos: DataWriterQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataWriterQos::default()
            },
            reader_qos: DataReaderQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        }
    }
}

/// Whether the remote endpoints `a` and `b` are announced as a pair: one
/// names the other as its related entity or, when neither names any, they
/// are of the same participant.
fn paired(shared: &Shared, a: &Guid, b: &Guid) -> bool {
    let rel = |g: &Guid| shared.endpoint_data(g).and_then(|d| d.related_entity_guid);
    match (rel(a), rel(b)) {
        (None, None) => a.prefix == b.prefix,
        (ra, rb) => ra == Some(*b) || rb == Some(*a),
    }
}

/// Announces `writer` and `reader` as a pair (DDS-RPC 1.0, 7.6.2.1).
fn pair<A, B>(
    writer: &DataWriter<A>,
    reader: &DataReader<B>,
    instance_name: Option<&String>,
) -> Result<()> {
    let (w, r) = (writer.guid, reader.guid);
    writer.shared.update_endpoint(&w, |d| {
        d.related_entity_guid = Some(r);
        d.service_instance_name = instance_name.cloned();
    })?;
    reader.shared.update_endpoint(&r, |d| {
        d.related_entity_guid = Some(w);
        d.service_instance_name = instance_name.cloned();
    })
}

/// Puts `clientid= <octets>;` at the head of the USER_DATA of a
/// requester's writer and reader, the octets being the writer's GUID in
/// hexadecimal, separated by dots: a service of rmw_cyclonedds_cpp
/// answers a request only when it has matched a reader with the
/// `clientid` of the request's writer (seen in captures, where its own
/// clients announce one).
fn announce_client_id<A, B>(writer: &DataWriter<A>, reader: &DataReader<B>) -> Result<()> {
    let g = writer.guid;
    let e = &g.entity_id;
    let octets: Vec<String> = g
        .prefix
        .0
        .iter()
        .chain(&e.entity_key)
        .chain([&e.entity_kind])
        .map(|b| format!("{b:x}"))
        .collect();
    let id = format!("clientid= {};", octets.join("."));
    let add = |d: &mut rustorb_rtps::sedp::EndpointData| {
        if !d.user_data.starts_with(b"clientid=") {
            d.user_data.splice(0..0, id.bytes());
        }
    };
    writer.shared.update_endpoint(&writer.guid, add)?;
    reader.shared.update_endpoint(&reader.guid, add)
}

/// Waits, a little at a time, until `f` gives something or `timeout`
/// passes.
fn wait_until<R>(timeout: Duration, mut f: impl FnMut() -> Option<R>) -> Option<R> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(r) = f() {
            return Some(r);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The client side of a service: sends requests of type `Req` and gets
/// the replies of type `Rep` to them (DDS-RPC 1.0, 7.8.1).
pub struct Requester<Req, Rep> {
    writer: DataWriter<Req>,
    reader: DataReader<Rep>,
    /// Replies taken from the reader and not yet given out, by the
    /// request they answer.
    replies: Mutex<HashMap<SampleIdentity, Vec<Rep>>>,
    /// The client id of the in-payload header.
    client_id: [u8; 8],
    /// The last sequence number of the in-payload header, and the
    /// requests sent with one by it.
    headers: Mutex<(i64, HashMap<i64, SampleIdentity>)>,
}

impl<Req, Rep> std::fmt::Debug for Requester<Req, Rep> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requester")
            .field("writer", &self.writer.guid)
            .field("reader", &self.reader.guid)
            .finish()
    }
}

impl<Req: TopicType + 'static, Rep: TopicType + 'static> Requester<Req, Rep> {
    /// A requester of the service `service`, on its default topics.
    pub fn new(participant: &DomainParticipant, service: &str) -> Result<Self> {
        Requester::with_config(participant, &RpcConfig::new(service))
    }

    /// A requester made as `config` says.
    pub fn with_config(participant: &DomainParticipant, config: &RpcConfig) -> Result<Self> {
        let request = participant.create_topic::<Req>(&config.request_topic)?;
        let reply = participant.create_topic::<Rep>(&config.reply_topic)?;
        Requester::with_topics(participant, &request, &reply, config)
    }
}

impl<Req, Rep> Requester<Req, Rep> {
    /// A requester on the topics given, of any sample type (such as
    /// [`DynamicData`](crate::DynamicData)), with the QoS of `config` (its
    /// topic names are not used).
    pub fn with_topics(
        participant: &DomainParticipant,
        request: &Topic<Req>,
        reply: &Topic<Rep>,
        config: &RpcConfig,
    ) -> Result<Self> {
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(request, config.writer_qos.clone())?;
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(reply, config.reader_qos.clone())?;
        pair(&writer, &reader, None)?;
        announce_client_id(&writer, &reader)?;
        // FNV-1a of the writer's GUID.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let e = &writer.guid.entity_id;
        for b in writer
            .guid
            .prefix
            .0
            .iter()
            .chain(&e.entity_key)
            .chain([&e.entity_kind])
        {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
        Ok(Requester {
            writer,
            reader,
            replies: Mutex::new(HashMap::new()),
            client_id: h.to_be_bytes(),
            headers: Mutex::new((0, HashMap::new())),
        })
    }

    /// The writer of the requests.
    pub fn request_datawriter(&self) -> &DataWriter<Req> {
        &self.writer
    }

    /// The reader of the replies.
    pub fn reply_datareader(&self) -> &DataReader<Rep> {
        &self.reader
    }

    /// Whether a replier is matched: a request reader matched by the
    /// request writer, and its reply writer matched by the reply reader
    /// (DDS-RPC 1.0, 7.6.2.2.2).
    pub fn is_service_matched(&self) -> bool {
        let shared = &self.writer.shared;
        let writers = self.reader.matched_publications();
        self.writer.matched_subscriptions().iter().any(|r| {
            writers.iter().any(|w| {
                // Cyclone DDS drops a request that comes before it
                // knows the reply reader with the request writer's
                // clientid, so wait until it has acknowledged the
                // latest announcements of both.
                paired(shared, r, w)
                    && (!is_cyclone(shared, w)
                        || [self.writer.guid, self.reader.guid]
                            .iter()
                            .all(|g| shared.is_announced_to(g, &w.prefix)))
            })
        })
    }

    /// Waits until [`is_service_matched`](Self::is_service_matched), or
    /// `timeout` passes; returns whether it is.
    pub fn wait_for_service(&self, timeout: Duration) -> bool {
        wait_until(timeout, || self.is_service_matched().then_some(())).is_some()
    }

    /// Sends a request; returns its identity, which its replies name.
    ///
    /// When a request reader of Cyclone DDS is matched, the request
    /// carries the in-payload header too.
    pub fn send_request(&self, request: &Req) -> Result<SampleIdentity> {
        let params = WriteParams {
            related_sample_identity: Some(SampleIdentity {
                writer_guid: self.reader.guid,
                sequence_number: SequenceNumber::UNKNOWN,
            }),
            ..WriteParams::default()
        };
        let shared = &self.writer.shared;
        let cyclone = self
            .writer
            .matched_subscriptions()
            .iter()
            .any(|r| is_cyclone(shared, r));
        if !cyclone {
            return self.writer.write_w_params(request, &params);
        }
        let mut headers = self.headers.lock().map_err(|_| Error::PreconditionNotMet)?;
        let seq = headers.0 + 1;
        let id = self
            .writer
            .write_mapped(request, &params, |p| insert_header(p, self.client_id, seq))?;
        headers.0 = seq;
        headers.1.insert(seq, id);
        Ok(id)
    }

    /// Moves the replies to this requester's requests from the reader to
    /// `replies`; drops those to others.
    fn collect(&self) -> Result<std::sync::MutexGuard<'_, HashMap<SampleIdentity, Vec<Rep>>>> {
        let mut replies = self.replies.lock().map_err(|_| Error::PreconditionNotMet)?;
        for (data, info, header) in take_with_headers(&self.reader) {
            let id = match header {
                // The header names the request.
                Some((client, seq)) => {
                    if client != self.client_id {
                        continue;
                    }
                    let mut headers = self.headers.lock().map_err(|_| Error::PreconditionNotMet)?;
                    headers.1.remove(&seq)
                }
                None => info.related_sample_identity,
            };
            let Some(mut id) = id else {
                continue;
            };
            // A reply names the request, or the reply reader with the
            // request's sequence number.
            if id.writer_guid == self.reader.guid {
                id.writer_guid = self.writer.guid;
            }
            if id.writer_guid == self.writer.guid {
                replies.entry(id).or_default().push(data);
            }
        }
        Ok(replies)
    }

    /// Takes the replies come so far to the request `request`.
    pub fn take_replies(&self, request: &SampleIdentity) -> Result<Vec<Rep>> {
        Ok(self.collect()?.remove(request).unwrap_or_default())
    }

    /// Takes the replies come so far to any request of this requester,
    /// with the request each answers.
    pub fn take_all_replies(&self) -> Result<Vec<(SampleIdentity, Rep)>> {
        let mut replies = self.collect()?;
        let mut all: Vec<(SampleIdentity, Rep)> = replies
            .drain()
            .flat_map(|(id, v)| v.into_iter().map(move |r| (id, r)))
            .collect();
        all.sort_by_key(|(id, _)| id.sequence_number);
        Ok(all)
    }

    /// Waits for a reply to `request` and takes it; fails with
    /// [`Error::Timeout`] when `timeout` passes first.
    pub fn receive_reply(&self, request: &SampleIdentity, timeout: Duration) -> Result<Rep> {
        wait_until(timeout, || match self.collect() {
            Ok(mut replies) => {
                let v = replies.get_mut(request)?;
                let r = v.remove(0);
                if v.is_empty() {
                    replies.remove(request);
                }
                Some(Ok(r))
            }
            Err(e) => Some(Err(e)),
        })
        .unwrap_or(Err(Error::Timeout))
    }

    /// Sends a request and waits for its first reply.
    pub fn request(&self, request: &Req, timeout: Duration) -> Result<Rep> {
        let id = self.send_request(request)?;
        self.receive_reply(&id, timeout)
    }
}

/// A request taken by a [`Replier`].
#[derive(Clone, Debug, PartialEq)]
pub struct Request<Req> {
    /// The request.
    pub data: Req,
    /// What the reply names: the request's writer and sequence number,
    /// or the reader the request named (as a ROS 2 client names its reply
    /// reader) with the request's sequence number.
    pub id: SampleIdentity,
    /// The writer of the request.
    pub writer: Guid,
}

/// The service side: takes requests of type `Req` and answers each with
/// replies of type `Rep` (DDS-RPC 1.0, 7.8.1).
pub struct Replier<Req, Rep> {
    reader: DataReader<Req>,
    writer: DataWriter<Rep>,
    /// Requests of requesters whose reply reader is not yet matched,
    /// held back (DDS-RPC 1.0, 7.6.2.2.1).
    held: Mutex<Vec<Request<Req>>>,
    /// The in-payload headers of the requests not yet answered, by the
    /// identity a reply names.
    headers: Mutex<HashMap<SampleIdentity, Header>>,
}

impl<Req, Rep> std::fmt::Debug for Replier<Req, Rep> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Replier")
            .field("reader", &self.reader.guid)
            .field("writer", &self.writer.guid)
            .finish()
    }
}

impl<Req: TopicType + 'static, Rep: TopicType + 'static> Replier<Req, Rep> {
    /// A replier of the service `service`, on its default topics.
    pub fn new(participant: &DomainParticipant, service: &str) -> Result<Self> {
        Replier::with_config(participant, &RpcConfig::new(service))
    }

    /// A replier made as `config` says.
    pub fn with_config(participant: &DomainParticipant, config: &RpcConfig) -> Result<Self> {
        let request = participant.create_topic::<Req>(&config.request_topic)?;
        let reply = participant.create_topic::<Rep>(&config.reply_topic)?;
        Replier::with_topics(participant, &request, &reply, config)
    }
}

impl<Req, Rep> Replier<Req, Rep> {
    /// A replier on the topics given, of any sample type (such as
    /// [`DynamicData`](crate::DynamicData)), with the QoS and the service
    /// instance name of `config` (its topic names are not used).
    pub fn with_topics(
        participant: &DomainParticipant,
        request: &Topic<Req>,
        reply: &Topic<Rep>,
        config: &RpcConfig,
    ) -> Result<Self> {
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(request, config.reader_qos.clone())?;
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(reply, config.writer_qos.clone())?;
        pair(&writer, &reader, config.service_instance_name.as_ref())?;
        Ok(Replier {
            reader,
            writer,
            held: Mutex::new(Vec::new()),
            headers: Mutex::new(HashMap::new()),
        })
    }

    /// The reader of the requests.
    pub fn request_datareader(&self) -> &DataReader<Req> {
        &self.reader
    }

    /// The writer of the replies.
    pub fn reply_datawriter(&self) -> &DataWriter<Rep> {
        &self.writer
    }

    /// Whether the reply writer has matched the reply reader of the
    /// requester whose request writer is `writer`.
    fn can_answer(&self, writer: &Guid) -> bool {
        let shared = &self.writer.shared;
        self.writer
            .matched_subscriptions()
            .iter()
            .any(|r| paired(shared, writer, r))
    }

    /// Takes the requests come so far, in order of arrival, but those of
    /// a requester whose reply reader is not yet matched: they stay until
    /// it is.
    pub fn take_requests(&self) -> Result<Vec<Request<Req>>> {
        let mut held = self.held.lock().map_err(|_| Error::PreconditionNotMet)?;
        for (data, info, header) in take_with_headers(&self.reader) {
            let named = info
                .related_sample_identity
                .map(|id| id.writer_guid)
                .filter(|g| *g != Guid::UNKNOWN);
            let id = SampleIdentity {
                writer_guid: named.unwrap_or(info.writer),
                sequence_number: info.sequence_number,
            };
            if let Some(h) = header {
                self.headers
                    .lock()
                    .map_err(|_| Error::PreconditionNotMet)?
                    .insert(id, h);
            }
            held.push(Request {
                data,
                id,
                writer: info.writer,
            });
        }
        let mut ready = Vec::new();
        let mut i = 0;
        while i < held.len() {
            if self.can_answer(&held[i].writer) {
                ready.push(held.remove(i));
            } else {
                i += 1;
            }
        }
        Ok(ready)
    }

    /// Waits for requests and takes them; an empty list when `timeout`
    /// passes first.
    pub fn receive_requests(&self, timeout: Duration) -> Result<Vec<Request<Req>>> {
        wait_until(timeout, || match self.take_requests() {
            Ok(v) if v.is_empty() => None,
            r => Some(r),
        })
        .unwrap_or(Ok(Vec::new()))
    }

    /// Sends `reply` to the request `request`; with the request's
    /// in-payload header at its head when it had one.
    pub fn send_reply(&self, reply: &Rep, request: &SampleIdentity) -> Result<()> {
        let params = WriteParams {
            related_sample_identity: Some(*request),
            ..WriteParams::default()
        };
        let header = self
            .headers
            .lock()
            .map_err(|_| Error::PreconditionNotMet)?
            .remove(request);
        match header {
            Some((client, seq)) => self
                .writer
                .write_mapped(reply, &params, |p| insert_header(p, client, seq)),
            None => self.writer.write_w_params(reply, &params),
        }
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_payload_header() {
        let client = [1, 2, 3, 4, 5, 6, 7, 8];
        for encap in [[0, 1, 0, 0], [0, 0, 0, 0]] {
            let mut payload = encap.to_vec();
            payload.extend_from_slice(&[9, 9, 9, 9]);
            let with = insert_header(payload.clone(), client, 42);
            assert_eq!(with.len(), payload.len() + HEADER);
            assert_eq!(&with[ENCAP..ENCAP + 8], &client);
            let seq = if encap[1] == 1 {
                42i64.to_le_bytes()
            } else {
                42i64.to_be_bytes()
            };
            assert_eq!(&with[ENCAP + 8..ENCAP + HEADER], &seq);
            assert_eq!(strip_header(&with), Some((client, 42, payload)));
        }
        assert_eq!(strip_header(&[0, 1, 0, 0, 1, 2]), None);
    }
}
