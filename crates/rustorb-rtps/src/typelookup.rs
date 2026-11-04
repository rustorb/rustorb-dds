//! The built-in endpoints of the TypeLookup service (DDS-XTypes 1.3
//! clause 7.6.3.3), with which participants ask each other for the
//! TypeObjects of the types their endpoints announce.
//!
//! [`TypeLookup`] holds the four endpoints of a participant, without
//! doing any I/O, like [`crate::sedp::Sedp`]. As a server it answers the
//! requests addressed to it from a [`TypeRegistry`]: getTypes with the
//! TypeObjects it has, getTypeDependencies with the types those depend
//! on. As a client it sends requests ([`TypeLookup::request`]) and hands
//! back the replies ([`TypeLookupReplied`]); what to do with them is the
//! caller's.
//!
//! Requests and replies go to every matched reader, as the endpoints have
//! no key: a server answers only the requests that name its instance
//! (7.6.3.3.4), and a client matches a reply to its request by the
//! request's identity. Some implementations put another identity in their
//! replies; a reply that matches no request is then taken for the oldest
//! request of the same operation to the participant that wrote it.

use std::collections::HashSet;
use std::time::Instant;

use rustorb_xtypes::registry::TypeRegistry;
use rustorb_xtypes::typelookup::{
    GetTypeDependenciesOut, GetTypesOut, REMOTE_EX_OK, REMOTE_EX_UNKNOWN_OPERATION, ReplyHeader,
    RequestHeader, SampleIdentity, TypeLookupCall, TypeLookupReply, TypeLookupRequest,
    TypeLookupReturn, instance_name,
};

use crate::receiver::receive;
use crate::reliable::{
    MatchedReader, Outgoing, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use crate::spdp::ParticipantData;
use crate::submessage::{PayloadKind, SerializedPayload, Submessage};
use crate::types::{BuiltinEndpointSet, EntityId, Guid, GuidPrefix, VendorId};

/// A reply to one of our requests.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeLookupReplied {
    /// The participant that replied.
    pub server: GuidPrefix,
    /// The request it answers, when it is known.
    pub call: Option<TypeLookupCall>,
    /// The reply.
    pub reply: TypeLookupReply,
}

#[derive(Debug)]
struct Pending {
    id: SampleIdentity,
    server: GuidPrefix,
    call: TypeLookupCall,
}

fn same_operation(a: &TypeLookupCall, b: &TypeLookupReturn) -> bool {
    matches!(
        (a, b),
        (TypeLookupCall::GetTypes(_), TypeLookupReturn::GetTypes(_))
            | (
                TypeLookupCall::GetTypeDependencies(_),
                TypeLookupReturn::GetTypeDependencies(_)
            )
    )
}

/// The TypeLookup service endpoints of one participant.
#[derive(Debug)]
pub struct TypeLookup {
    prefix: GuidPrefix,
    /// Our instance name, which requests to us carry.
    instance_name: String,
    request_writer: ReliableWriter,
    request_reader: ReliableReader,
    reply_writer: ReliableWriter,
    reply_reader: ReliableReader,
    /// Our requests not answered yet, oldest first.
    pending: Vec<Pending>,
}

impl TypeLookup {
    /// The bits of `availableBuiltinEndpoints` for the endpoints provided
    /// here (DDS-XTypes 1.3 Table 62).
    pub const BUILTIN_ENDPOINTS: u32 = BuiltinEndpointSet::TYPE_LOOKUP_REQUEST_WRITER
        | BuiltinEndpointSet::TYPE_LOOKUP_REQUEST_READER
        | BuiltinEndpointSet::TYPE_LOOKUP_REPLY_WRITER
        | BuiltinEndpointSet::TYPE_LOOKUP_REPLY_READER;

    /// The endpoints of the participant `prefix`. They are RELIABLE,
    /// KEEP_ALL and VOLATILE (7.6.3.3.3).
    pub fn new(
        prefix: GuidPrefix,
        vendor_id: VendorId,
        writer: WriterConfig,
        reader: ReaderConfig,
    ) -> Self {
        let guid = |e| Guid::new(prefix, e);
        TypeLookup {
            prefix,
            instance_name: instance_name(&guid(EntityId::PARTICIPANT).to_bytes()),
            request_writer: ReliableWriter::new(
                guid(EntityId::TL_SVC_REQ_WRITER),
                vendor_id,
                writer.clone(),
            ),
            request_reader: ReliableReader::new(
                guid(EntityId::TL_SVC_REQ_READER),
                vendor_id,
                reader.clone(),
            ),
            reply_writer: ReliableWriter::new(
                guid(EntityId::TL_SVC_REPLY_WRITER),
                vendor_id,
                writer,
            ),
            reply_reader: ReliableReader::new(
                guid(EntityId::TL_SVC_REPLY_READER),
                vendor_id,
                reader,
            ),
            pending: Vec::new(),
        }
    }

    /// Whether the participant `p` serves TypeLookup requests: it has the
    /// request reader and the reply writer.
    pub fn is_server(p: &ParticipantData) -> bool {
        p.available_builtin_endpoints.contains(
            BuiltinEndpointSet::TYPE_LOOKUP_REQUEST_READER
                | BuiltinEndpointSet::TYPE_LOOKUP_REPLY_WRITER,
        )
    }

    /// Connects with the endpoints of a discovered participant, as far
    /// as its `availableBuiltinEndpoints` says it has them. The local
    /// participant is ignored.
    pub fn add_participant(&mut self, p: &ParticipantData, now: Instant) {
        if p.guid_prefix == self.prefix {
            return;
        }
        let set = p.available_builtin_endpoints;
        let uc = p.metatraffic_unicast_locators.clone();
        let mc = p.metatraffic_multicast_locators.clone();
        let reader = |uc: &Vec<_>, mc: &Vec<_>| MatchedReader {
            unicast: uc.clone(),
            multicast: mc.clone(),
            reliable: true,
            history: false,
        };
        let guid = |e| Guid::new(p.guid_prefix, e);
        if set.contains(BuiltinEndpointSet::TYPE_LOOKUP_REQUEST_READER) {
            self.request_writer
                .add_matched_reader(guid(EntityId::TL_SVC_REQ_READER), reader(&uc, &mc));
        }
        if set.contains(BuiltinEndpointSet::TYPE_LOOKUP_REPLY_READER) {
            self.reply_writer
                .add_matched_reader(guid(EntityId::TL_SVC_REPLY_READER), reader(&uc, &mc));
        }
        if set.contains(BuiltinEndpointSet::TYPE_LOOKUP_REQUEST_WRITER) {
            self.request_reader.add_writer(
                guid(EntityId::TL_SVC_REQ_WRITER),
                uc.clone(),
                mc.clone(),
                now,
            );
        }
        if set.contains(BuiltinEndpointSet::TYPE_LOOKUP_REPLY_WRITER) {
            self.reply_reader
                .add_writer(guid(EntityId::TL_SVC_REPLY_WRITER), uc, mc, now);
        }
    }

    /// Disconnects from a participant that is gone, and forgets the
    /// requests made to it.
    pub fn remove_participant(&mut self, prefix: &GuidPrefix) {
        self.request_writer.remove_readers_of(prefix);
        self.reply_writer.remove_readers_of(prefix);
        self.request_reader.remove_writers_of(prefix);
        self.reply_reader.remove_writers_of(prefix);
        self.pending.retain(|p| &p.server != prefix);
    }

    /// Sends `call` to the participant `server`. It goes at the next
    /// [`Self::poll`]; the reply comes from [`Self::handle_message`].
    pub fn request(&mut self, server: GuidPrefix, call: TypeLookupCall) -> SampleIdentity {
        let id = SampleIdentity {
            writer_guid: self.request_writer.guid().to_bytes(),
            sequence_number: self.request_writer.last_sn().0 + 1,
        };
        let request = TypeLookupRequest {
            header: RequestHeader {
                request_id: id,
                instance_name: instance_name(&Guid::new(server, EntityId::PARTICIPANT).to_bytes()),
            },
            data: call.clone(),
        };
        if let Ok(bytes) = request.to_payload() {
            let sn = self.request_writer.add_change(
                None,
                Some(SerializedPayload {
                    kind: PayloadKind::Data,
                    bytes,
                }),
            );
            debug_assert_eq!(sn.0, id.sequence_number);
            self.pending.push(Pending { id, server, call });
        }
        id
    }

    /// Whether a getTypes request for `id` is waiting for its reply.
    pub fn is_requested(&self, id: &rustorb_xtypes::type_object::TypeIdentifier) -> bool {
        self.pending.iter().any(|p| match &p.call {
            TypeLookupCall::GetTypes(c) => c.type_ids.contains(id),
            _ => false,
        })
    }

    /// The number of requests not answered yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Processes one received RTPS message: answers the requests to this
    /// participant from `registry`, and returns the replies to ours.
    pub fn handle_message(
        &mut self,
        bytes: &[u8],
        now: Instant,
        registry: &TypeRegistry,
    ) -> Vec<TypeLookupReplied> {
        let mut out = Vec::new();
        let Some((_, received)) = receive(bytes, self.prefix) else {
            return out;
        };
        for r in received {
            let src = r.source_prefix;
            let (requests, replies) = match r.submessage {
                Submessage::Data(d) => {
                    let w = Guid::new(src, d.writer_id);
                    match d.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            (self.request_reader.handle_data(w, &d, r.timestamp), vec![])
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            (vec![], self.reply_reader.handle_data(w, &d, r.timestamp))
                        }
                        _ => continue,
                    }
                }
                Submessage::DataFrag(d) => {
                    let w = Guid::new(src, d.writer_id);
                    match d.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => (
                            self.request_reader.handle_data_frag(w, &d, r.timestamp),
                            vec![],
                        ),
                        EntityId::TL_SVC_REPLY_WRITER => (
                            vec![],
                            self.reply_reader.handle_data_frag(w, &d, r.timestamp),
                        ),
                        _ => continue,
                    }
                }
                Submessage::Gap(g) => {
                    let w = Guid::new(src, g.writer_id);
                    match g.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            (self.request_reader.handle_gap(w, &g), vec![])
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            (vec![], self.reply_reader.handle_gap(w, &g))
                        }
                        _ => continue,
                    }
                }
                Submessage::Heartbeat(h) => {
                    let w = Guid::new(src, h.writer_id);
                    match h.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            (self.request_reader.handle_heartbeat(w, &h, now), vec![])
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            (vec![], self.reply_reader.handle_heartbeat(w, &h, now))
                        }
                        _ => continue,
                    }
                }
                Submessage::HeartbeatFrag(h) => {
                    let w = Guid::new(src, h.writer_id);
                    match h.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            self.request_reader.handle_heartbeat_frag(w, &h, now)
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            self.reply_reader.handle_heartbeat_frag(w, &h, now)
                        }
                        _ => {}
                    }
                    continue;
                }
                Submessage::AckNack(a) => {
                    let reader = Guid::new(src, a.reader_id);
                    match a.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            self.request_writer.handle_acknack(reader, &a, now)
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            self.reply_writer.handle_acknack(reader, &a, now)
                        }
                        _ => {}
                    }
                    continue;
                }
                Submessage::NackFrag(n) => {
                    let reader = Guid::new(src, n.reader_id);
                    match n.writer_id {
                        EntityId::TL_SVC_REQ_WRITER => {
                            self.request_writer.handle_nackfrag(reader, &n, now)
                        }
                        EntityId::TL_SVC_REPLY_WRITER => {
                            self.reply_writer.handle_nackfrag(reader, &n, now)
                        }
                        _ => {}
                    }
                    continue;
                }
                _ => continue,
            };
            for c in requests {
                let Some(p) = c.payload.filter(|p| p.kind == PayloadKind::Data) else {
                    continue;
                };
                if let Ok(request) = TypeLookupRequest::from_payload(&p.bytes) {
                    self.serve(request, registry);
                }
            }
            for c in replies {
                let Some(p) = c.payload.filter(|p| p.kind == PayloadKind::Data) else {
                    continue;
                };
                if let Ok(reply) = TypeLookupReply::from_payload(&p.bytes) {
                    out.extend(self.replied(c.writer.prefix, reply));
                }
            }
        }
        out
    }

    /// Answers a request, if it is to us.
    fn serve(&mut self, request: TypeLookupRequest, registry: &TypeRegistry) {
        if request.header.instance_name != self.instance_name {
            return;
        }
        let (remote_ex, data) = match request.data {
            TypeLookupCall::GetTypes(call) => {
                let mut seen = HashSet::new();
                let types = call
                    .type_ids
                    .iter()
                    .filter(|id| seen.insert(*id))
                    .filter_map(|id| {
                        registry.get(id).map(|o| {
                            rustorb_xtypes::type_object::TypeIdentifierTypeObjectPair {
                                type_identifier: id.clone(),
                                type_object: o.clone(),
                            }
                        })
                    })
                    .collect();
                (
                    REMOTE_EX_OK,
                    TypeLookupReturn::GetTypes(Ok(GetTypesOut {
                        types,
                        complete_to_minimal: Vec::new(),
                    })),
                )
            }
            TypeLookupCall::GetTypeDependencies(call) => {
                // All of them at once: no continuation point.
                let asked: HashSet<_> = call.type_ids.iter().collect();
                let mut seen = HashSet::new();
                let dependent_typeids = call
                    .type_ids
                    .iter()
                    .flat_map(|id| registry.dependencies(id).unwrap_or_default())
                    .filter(|id| !asked.contains(id) && seen.insert(id.clone()))
                    .filter_map(|id| registry.with_size(&id))
                    .collect();
                (
                    REMOTE_EX_OK,
                    TypeLookupReturn::GetTypeDependencies(Ok(GetTypeDependenciesOut {
                        dependent_typeids,
                        continuation_point: None,
                    })),
                )
            }
            TypeLookupCall::Unknown(d) => {
                (REMOTE_EX_UNKNOWN_OPERATION, TypeLookupReturn::Unknown(d))
            }
        };
        let reply = TypeLookupReply {
            header: ReplyHeader {
                related_request_id: request.header.request_id,
                remote_ex,
            },
            data,
        };
        if let Ok(bytes) = reply.to_payload() {
            self.reply_writer.add_change(
                None,
                Some(SerializedPayload {
                    kind: PayloadKind::Data,
                    bytes,
                }),
            );
        }
    }

    /// Matches a reply from `server` with one of our requests.
    fn replied(&mut self, server: GuidPrefix, reply: TypeLookupReply) -> Option<TypeLookupReplied> {
        let id = reply.header.related_request_id;
        let at = match self.pending.iter().position(|p| p.id == id) {
            Some(at) => Some(at),
            // A reply to another participant's request.
            None if id.writer_guid[..12] == self.prefix.0 => return None,
            None => self
                .pending
                .iter()
                .position(|p| p.server == server && same_operation(&p.call, &reply.data)),
        };
        let call = at.map(|at| self.pending.remove(at).call);
        if call.is_none() && server == self.prefix {
            return None;
        }
        Some(TypeLookupReplied {
            server,
            call,
            reply,
        })
    }

    /// The datagrams due at `now`.
    pub fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        // VOLATILE: what every reader has is no longer needed.
        self.request_writer.remove_acknowledged();
        self.reply_writer.remove_acknowledged();
        let mut out = self.request_writer.poll(now);
        out.extend(self.reply_writer.poll(now));
        out.extend(self.request_reader.poll(now));
        out.extend(self.reply_reader.poll(now));
        out
    }

    /// When [`Self::poll`] next has something to do, if no message comes
    /// in before.
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.request_writer.next_deadline(),
            self.reply_writer.next_deadline(),
            self.request_reader.next_deadline(),
            self.reply_reader.next_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Locator;
    use rustorb_xtypes::type_object::TypeIdentifier;
    use rustorb_xtypes::typelookup::{GetTypeDependenciesIn, GetTypesIn};
    use rustorb_xtypes::types::*;

    fn prefix(n: u8) -> GuidPrefix {
        GuidPrefix([n; 12])
    }

    fn participant(n: u8) -> ParticipantData {
        let mut p = ParticipantData::new(prefix(n), VendorId::UNKNOWN);
        p.available_builtin_endpoints.0 |= TypeLookup::BUILTIN_ENDPOINTS;
        p.metatraffic_unicast_locators = vec![Locator::udpv4(std::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            7400 + n as u16,
        ))];
        p
    }

    fn tl(n: u8) -> TypeLookup {
        TypeLookup::new(
            prefix(n),
            VendorId::UNKNOWN,
            WriterConfig::default(),
            ReaderConfig::default(),
        )
    }

    /// Delivers everything due between the two until both are quiet.
    fn pump(
        a: &mut TypeLookup,
        ra: &TypeRegistry,
        b: &mut TypeLookup,
        rb: &TypeRegistry,
        t: Instant,
    ) -> (Vec<TypeLookupReplied>, Vec<TypeLookupReplied>) {
        let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
        for _ in 0..10 {
            let out_a = a.poll(t);
            let out_b = b.poll(t);
            if out_a.is_empty() && out_b.is_empty() {
                break;
            }
            for o in out_a {
                got_b.extend(b.handle_message(&o.bytes, t, rb));
            }
            for o in out_b {
                got_a.extend(a.handle_message(&o.bytes, t, ra));
            }
        }
        (got_a, got_b)
    }

    fn nested() -> TypeObjects {
        let inner: Type = StructType {
            name: "M::Inner".into(),
            members: vec![StructMember::new("x", 0, Type::Primitive(Primitive::Int32))],
            ..Default::default()
        }
        .into();
        let outer: Type = StructType {
            name: "M::Outer".into(),
            members: vec![StructMember::new("i", 0, inner)],
            ..Default::default()
        }
        .into();
        TypeObjects::new(&outer).unwrap()
    }

    #[test]
    fn types_and_dependencies_are_served() {
        let t = Instant::now();
        let objects = nested();
        let mut server_types = TypeRegistry::new();
        server_types.add_type(&objects);
        let client_types = TypeRegistry::new();
        let (mut a, mut b) = (tl(1), tl(2));
        a.add_participant(&participant(2), t);
        b.add_participant(&participant(1), t);

        let root = objects.minimal_id().clone();
        let inner = objects.information.minimal.dependent_typeids[0].clone();
        a.request(
            prefix(2),
            TypeLookupCall::GetTypeDependencies(GetTypeDependenciesIn {
                type_ids: vec![root.clone()],
                continuation_point: None,
            }),
        );
        let unknown = TypeIdentifier::Minimal([9; 14]);
        a.request(
            prefix(2),
            TypeLookupCall::GetTypes(GetTypesIn {
                type_ids: vec![root.clone(), unknown],
            }),
        );
        assert_eq!(a.pending(), 2);
        let (got, other) = pump(&mut a, &client_types, &mut b, &server_types, t);
        assert!(other.is_empty());
        assert_eq!(got.len(), 2);
        assert_eq!(a.pending(), 0);
        let TypeLookupReturn::GetTypeDependencies(Ok(deps)) = &got[0].reply.data else {
            panic!("{:?}", got[0]);
        };
        assert_eq!(deps.dependent_typeids, [inner]);
        assert!(matches!(
            got[0].call,
            Some(TypeLookupCall::GetTypeDependencies(_))
        ));
        let TypeLookupReturn::GetTypes(Ok(types)) = &got[1].reply.data else {
            panic!("{:?}", got[1]);
        };
        // The unknown one is left out.
        assert_eq!(types.types.len(), 1);
        assert_eq!(types.types[0].type_identifier, root);
        assert_eq!(got[1].server, prefix(2));

        // Requests to another participant are not answered.
        let mut c = tl(3);
        c.add_participant(&participant(1), t);
        a.add_participant(&participant(3), t);
        a.request(
            prefix(4),
            TypeLookupCall::GetTypes(GetTypesIn {
                type_ids: vec![root],
            }),
        );
        let (got, _) = pump(&mut a, &client_types, &mut c, &server_types, t);
        assert!(got.is_empty());
        assert_eq!(a.pending(), 1);
        a.remove_participant(&prefix(4));
        assert_eq!(a.pending(), 0);
    }

    #[test]
    fn replies_with_another_identity_are_matched_by_operation() {
        let mut a = tl(1);
        let call = TypeLookupCall::GetTypes(GetTypesIn {
            type_ids: vec![TypeIdentifier::Minimal([1; 14])],
        });
        a.request(prefix(2), call.clone());
        let reply = |guid: [u8; 16]| TypeLookupReply {
            header: ReplyHeader {
                related_request_id: SampleIdentity {
                    writer_guid: guid,
                    sequence_number: 1,
                },
                remote_ex: REMOTE_EX_OK,
            },
            data: TypeLookupReturn::GetTypes(Ok(GetTypesOut::default())),
        };
        // From a participant we did not ask: handed on, with no request.
        assert!(
            a.replied(prefix(3), reply([7; 16]))
                .is_some_and(|r| r.call.is_none())
        );
        // As Cyclone DDS 0.10.4 answers: its own reply writer as the
        // identity.
        let cyclone = Guid::new(prefix(2), EntityId::TL_SVC_REPLY_WRITER).to_bytes();
        let got = a.replied(prefix(2), reply(cyclone)).unwrap();
        assert_eq!(got.call, Some(call));
        assert_eq!(a.pending(), 0);
        // A late reply to a request of ours that was answered already.
        let ours = Guid::new(prefix(1), EntityId::TL_SVC_REQ_WRITER).to_bytes();
        assert!(a.replied(prefix(2), reply(ours)).is_none());
    }
}
