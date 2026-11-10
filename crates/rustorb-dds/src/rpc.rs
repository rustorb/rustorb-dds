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
//! Each side announces its writer and reader as a pair through discovery,
//! with `PID_RELATED_ENTITY_GUID` (7.6.2.1), and holds back what would
//! go astray while the other side's pair is half discovered (7.6.2.2): a
//! [`Replier`] does not hand out the requests of a requester until it has
//! matched that requester's reply reader, and
//! [`Requester::wait_for_service`] waits for a replier both of whose
//! endpoints are matched. A remote endpoint that does not announce a
//! related entity is paired with those of the same participant.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rustorb_rtps::{Guid, SampleIdentity};
use rustorb_typesupport::TopicType;

use crate::participant::Shared;
use crate::publisher::{DataWriter, WriteParams};
use crate::qos::{DataReaderQos, DataWriterQos, History, Reliability};
use crate::subscriber::DataReader;
use crate::{DomainParticipant, Error, Result};

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
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(&request, config.writer_qos.clone())?;
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(&reply, config.reader_qos.clone())?;
        pair(&writer, &reader, None)?;
        Ok(Requester {
            writer,
            reader,
            replies: Mutex::new(HashMap::new()),
        })
    }
}

impl<Req, Rep> Requester<Req, Rep> {
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
        self.writer
            .matched_subscriptions()
            .iter()
            .any(|r| writers.iter().any(|w| paired(shared, r, w)))
    }

    /// Waits until [`is_service_matched`](Self::is_service_matched), or
    /// `timeout` passes; returns whether it is.
    pub fn wait_for_service(&self, timeout: Duration) -> bool {
        wait_until(timeout, || self.is_service_matched().then_some(())).is_some()
    }

    /// Sends a request; returns its identity, which its replies name.
    pub fn send_request(&self, request: &Req) -> Result<SampleIdentity> {
        self.writer.write_w_params(request, &WriteParams::default())
    }

    /// Moves the replies to this requester's requests from the reader to
    /// `replies`; drops those to others.
    fn collect(&self) -> Result<std::sync::MutexGuard<'_, HashMap<SampleIdentity, Vec<Rep>>>> {
        let mut replies = self.replies.lock().map_err(|_| Error::PreconditionNotMet)?;
        for s in self.reader.take() {
            let (Some(data), Some(id)) = (s.data, s.info.related_sample_identity) else {
                continue;
            };
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
    /// Its identity, which the reply names.
    pub id: SampleIdentity,
}

/// The service side: takes requests of type `Req` and answers each with
/// replies of type `Rep` (DDS-RPC 1.0, 7.8.1).
pub struct Replier<Req, Rep> {
    reader: DataReader<Req>,
    writer: DataWriter<Rep>,
    /// Requests of requesters whose reply reader is not yet matched,
    /// held back (DDS-RPC 1.0, 7.6.2.2.1).
    held: Mutex<Vec<Request<Req>>>,
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
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(&request, config.reader_qos.clone())?;
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(&reply, config.writer_qos.clone())?;
        pair(&writer, &reader, config.service_instance_name.as_ref())?;
        Ok(Replier {
            reader,
            writer,
            held: Mutex::new(Vec::new()),
        })
    }
}

impl<Req, Rep> Replier<Req, Rep> {
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
        held.extend(self.reader.take().into_iter().filter_map(|s| {
            Some(Request {
                data: s.data?,
                id: SampleIdentity {
                    writer_guid: s.info.writer,
                    sequence_number: s.info.sequence_number,
                },
            })
        }));
        let mut ready = Vec::new();
        let mut i = 0;
        while i < held.len() {
            if self.can_answer(&held[i].id.writer_guid) {
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

    /// Sends `reply` to the request `request`.
    pub fn send_reply(&self, reply: &Rep, request: &SampleIdentity) -> Result<()> {
        self.writer
            .write_w_params(
                reply,
                &WriteParams {
                    related_sample_identity: Some(*request),
                    ..WriteParams::default()
                },
            )
            .map(|_| ())
    }
}
