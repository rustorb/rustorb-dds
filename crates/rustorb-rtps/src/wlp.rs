//! The Writer Liveliness Protocol (DDSI-RTPS 2.5, 8.4.13 and 9.6.3.1).
//!
//! A participant asserts the liveliness of its AUTOMATIC and
//! MANUAL_BY_PARTICIPANT writers by writing a [`ParticipantMessage`] with
//! its built-in participant message writer; the other participants read
//! it with their built-in participant message reader. Both endpoints are
//! RELIABLE, TRANSIENT_LOCAL and KEEP_LAST 1 (8.4.13.3): the history
//! keeps the last message of each kind.
//!
//! [`Wlp`] holds the two endpoints, without doing any I/O, like
//! [`crate::sedp::Sedp`]. The caller says when to assert
//! ([`Wlp::assert_liveliness`]) and learns from [`Wlp::handle_message`]
//! which participants asserted what. MANUAL_BY_TOPIC liveliness does not
//! use it: it goes with the writer's own DATA and HEARTBEAT (8.7.2.2.3).

use std::collections::HashMap;
use std::time::Instant;

use crate::error::DecodeError;
use crate::parameter::{Parameter, ParameterList};
use crate::pl::{PL_CDR_BE, PL_CDR_LE, pid};
use crate::receiver::receive;
use crate::reliable::{
    MatchedReader, Outgoing, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use crate::spdp::ParticipantData;
use crate::submessage::{PayloadKind, SerializedPayload, Submessage};
use crate::types::{BuiltinEndpointSet, EntityId, Guid, GuidPrefix, SequenceNumber, VendorId};
use crate::wire::Endianness;

/// `ParticipantMessageData::kind` values (9.6.3.1).
pub mod kind {
    /// `PARTICIPANT_MESSAGE_DATA_KIND_UNKNOWN`.
    pub const UNKNOWN: [u8; 4] = [0, 0, 0, 0];
    /// `PARTICIPANT_MESSAGE_DATA_KIND_AUTOMATIC_LIVELINESS_UPDATE`.
    pub const AUTOMATIC_LIVELINESS_UPDATE: [u8; 4] = [0, 0, 0, 1];
    /// `PARTICIPANT_MESSAGE_DATA_KIND_MANUAL_LIVELINESS_UPDATE`.
    pub const MANUAL_LIVELINESS_UPDATE: [u8; 4] = [0, 0, 0, 2];
}

/// The `BEST_EFFORT_PARTICIPANT_MESSAGE_DATA_READER` bit of
/// `builtinEndpointQos` (9.3.2.12, 8.4.13.3).
pub const BEST_EFFORT_PARTICIPANT_MESSAGE_DATA_READER: u32 = 1 << 0;

/// `CDR_BE` and `CDR_LE` (Table 10.1).
const CDR_BE: [u8; 2] = [0x00, 0x00];
const CDR_LE: [u8; 2] = [0x00, 0x01];

/// `ParticipantMessageData` (9.6.3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticipantMessage {
    /// `participantGuidPrefix`: the participant that writes it.
    pub participant: GuidPrefix,
    /// `kind`: what it asserts (see [`kind`]).
    pub kind: [u8; 4],
    /// `data`.
    pub data: Vec<u8>,
}

impl ParticipantMessage {
    /// The serialized payload: the CDR encapsulation header, then the
    /// prefix, the kind and the data as a sequence of octets.
    pub fn to_payload(&self, endianness: Endianness) -> Vec<u8> {
        let mut b = Vec::with_capacity(4 + 20 + self.data.len());
        b.extend_from_slice(match endianness {
            Endianness::Big => &CDR_BE,
            Endianness::Little => &CDR_LE,
        });
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&self.participant.0);
        b.extend_from_slice(&self.kind);
        let n = self.data.len() as u32;
        b.extend_from_slice(&match endianness {
            Endianness::Big => n.to_be_bytes(),
            Endianness::Little => n.to_le_bytes(),
        });
        b.extend_from_slice(&self.data);
        b
    }

    /// Reads a serialized payload.
    pub fn from_payload(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < 4 + 20 {
            return Err(DecodeError::UnexpectedEnd);
        }
        // Fast DDS 2.11.2 marks it PL_CDR_LE, though what follows is
        // plain CDR: the PL_CDR identifiers are taken as CDR too.
        let big = match [bytes[0], bytes[1]] {
            CDR_BE | PL_CDR_BE => true,
            CDR_LE | PL_CDR_LE => false,
            other => return Err(DecodeError::UnknownRepresentation(other)),
        };
        let body = &bytes[4..];
        let participant = GuidPrefix(body[..12].try_into().expect("12 octets"));
        let kind = body[12..16].try_into().expect("4 octets");
        let len: [u8; 4] = body[16..20].try_into().expect("4 octets");
        let len = if big {
            u32::from_be_bytes(len)
        } else {
            u32::from_le_bytes(len)
        } as usize;
        let data = body
            .get(20..20 + len)
            .ok_or(DecodeError::UnexpectedEnd)?
            .to_vec();
        Ok(ParticipantMessage {
            participant,
            kind,
            data,
        })
    }

    /// The key hash of its instance: the key (prefix and kind) is 16
    /// octets, so it is the key itself (9.6.4.8).
    pub fn key_hash(&self) -> [u8; 16] {
        let mut h = [0u8; 16];
        h[..12].copy_from_slice(&self.participant.0);
        h[12..].copy_from_slice(&self.kind);
        h
    }
}

/// A remote participant asserted liveliness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LivelinessAsserted {
    /// The participant.
    pub participant: GuidPrefix,
    /// The kind of its message (see [`kind`]).
    pub kind: [u8; 4],
}

/// The built-in participant message writer and reader of one participant.
#[derive(Debug)]
pub struct Wlp {
    prefix: GuidPrefix,
    /// The entity ids of the writer and the reader, and their bits.
    ids: (EntityId, EntityId),
    bits: (u32, u32),
    writer: ReliableWriter,
    reader: ReliableReader,
    /// The change in the history for each kind (KEEP_LAST 1).
    last: HashMap<[u8; 4], SequenceNumber>,
}

impl Wlp {
    /// The bits of `availableBuiltinEndpoints` for the endpoints provided
    /// here (9.3.2.12).
    pub const BUILTIN_ENDPOINTS: u32 = BuiltinEndpointSet::PARTICIPANT_MESSAGE_DATA_WRITER
        | BuiltinEndpointSet::PARTICIPANT_MESSAGE_DATA_READER;

    /// The bits of the secure endpoints (DDS Security 1.2, 7.4.7.1).
    pub const BUILTIN_ENDPOINTS_SECURE: u32 = BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_WRITER
        | BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_READER;

    /// The endpoints of the participant `prefix`.
    pub fn new(
        prefix: GuidPrefix,
        vendor_id: VendorId,
        writer: WriterConfig,
        reader: ReaderConfig,
    ) -> Self {
        Self::with_ids(
            prefix,
            vendor_id,
            writer,
            reader,
            (
                EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_WRITER,
                EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_READER,
            ),
            (
                BuiltinEndpointSet::PARTICIPANT_MESSAGE_DATA_WRITER,
                BuiltinEndpointSet::PARTICIPANT_MESSAGE_DATA_READER,
            ),
        )
    }

    /// The secure endpoints of the participant `prefix`
    /// (BuiltinParticipantMessageSecureWriter and Reader, DDS Security
    /// 1.2, 7.4.5), whose messages DDS Security protects.
    pub fn new_secure(
        prefix: GuidPrefix,
        vendor_id: VendorId,
        writer: WriterConfig,
        reader: ReaderConfig,
    ) -> Self {
        Self::with_ids(
            prefix,
            vendor_id,
            writer,
            reader,
            (
                EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_WRITER,
                EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_READER,
            ),
            (
                BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_WRITER,
                BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_READER,
            ),
        )
    }

    fn with_ids(
        prefix: GuidPrefix,
        vendor_id: VendorId,
        writer: WriterConfig,
        reader: ReaderConfig,
        ids: (EntityId, EntityId),
        bits: (u32, u32),
    ) -> Self {
        Wlp {
            prefix,
            ids,
            bits,
            writer: ReliableWriter::new(Guid::new(prefix, ids.0), vendor_id, writer),
            reader: ReliableReader::new(Guid::new(prefix, ids.1), vendor_id, reader),
            last: HashMap::new(),
        }
    }

    /// Connects with the endpoints of a discovered participant, as far
    /// as its `availableBuiltinEndpoints` says it has them. Its reader is
    /// served best-effort when its `builtinEndpointQos` says so, reliably
    /// otherwise (8.4.13.3). The local participant is ignored.
    pub fn add_participant(&mut self, p: &ParticipantData, now: Instant) {
        if p.guid_prefix == self.prefix {
            return;
        }
        let set = p.available_builtin_endpoints;
        let uc = p.metatraffic_unicast_locators.clone();
        let mc = p.metatraffic_multicast_locators.clone();
        if set.contains(self.bits.1) {
            let best_effort = p
                .builtin_endpoint_qos
                .is_some_and(|q| q & BEST_EFFORT_PARTICIPANT_MESSAGE_DATA_READER != 0);
            self.writer.add_matched_reader(
                Guid::new(p.guid_prefix, self.ids.1),
                MatchedReader {
                    unicast: uc.clone(),
                    multicast: mc.clone(),
                    reliable: !best_effort,
                    history: true,
                },
            );
        }
        if set.contains(self.bits.0) {
            self.reader
                .add_writer(Guid::new(p.guid_prefix, self.ids.0), uc, mc, now);
        }
    }

    /// Disconnects from a participant that is gone.
    pub fn remove_participant(&mut self, prefix: &GuidPrefix) {
        self.writer.remove_readers_of(prefix);
        self.reader.remove_writers_of(prefix);
    }

    /// Writes a message of kind `kind` (see [`kind`]), replacing the last
    /// one of that kind. It is sent at the next [`Self::poll`].
    pub fn assert_liveliness(&mut self, kind: [u8; 4]) {
        let m = ParticipantMessage {
            participant: self.prefix,
            kind,
            data: Vec::new(),
        };
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, m.key_hash()));
        let sn = self.writer.add_change(
            Some(qos),
            Some(SerializedPayload {
                kind: PayloadKind::Data,
                bytes: m.to_payload(Endianness::NATIVE),
            }),
        );
        if let Some(old) = self.last.insert(kind, sn) {
            self.writer.remove_change(old);
        }
    }

    /// Processes one received RTPS message: returns the messages of other
    /// participants it delivers.
    pub fn handle_message(&mut self, bytes: &[u8], now: Instant) -> Vec<LivelinessAsserted> {
        let mut out = Vec::new();
        let Some((_, received)) = receive(bytes, self.prefix) else {
            return out;
        };
        let w = self.ids.0;
        for r in received {
            let src = r.source_prefix;
            let changes = match r.submessage {
                Submessage::Data(d) if d.writer_id == w => {
                    self.reader
                        .handle_data(Guid::new(src, d.writer_id), &d, r.timestamp)
                }
                Submessage::DataFrag(d) if d.writer_id == w => {
                    self.reader
                        .handle_data_frag(Guid::new(src, d.writer_id), &d, r.timestamp)
                }
                Submessage::Gap(g) if g.writer_id == w => {
                    self.reader.handle_gap(Guid::new(src, g.writer_id), &g)
                }
                Submessage::Heartbeat(h) if h.writer_id == w => {
                    self.reader
                        .handle_heartbeat(Guid::new(src, h.writer_id), &h, now)
                }
                Submessage::HeartbeatFrag(h) if h.writer_id == w => {
                    self.reader
                        .handle_heartbeat_frag(Guid::new(src, h.writer_id), &h, now);
                    Vec::new()
                }
                Submessage::AckNack(a) if a.writer_id == w => {
                    self.writer
                        .handle_acknack(Guid::new(src, a.reader_id), &a, now);
                    Vec::new()
                }
                Submessage::NackFrag(n) if n.writer_id == w => {
                    self.writer
                        .handle_nackfrag(Guid::new(src, n.reader_id), &n, now);
                    Vec::new()
                }
                _ => Vec::new(),
            };
            for c in changes {
                let Some(p) = c.payload.filter(|p| p.kind == PayloadKind::Data) else {
                    continue;
                };
                // The writer's participant is the one asserting, whatever
                // the payload says.
                if let Ok(m) = ParticipantMessage::from_payload(&p.bytes) {
                    out.push(LivelinessAsserted {
                        participant: c.writer.prefix,
                        kind: m.kind,
                    });
                }
            }
        }
        out
    }

    /// The datagrams due at `now`.
    pub fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        let mut out = self.writer.poll(now);
        out.extend(self.reader.poll(now));
        out
    }

    /// When [`Self::poll`] next has something to do, if no message comes
    /// in before.
    pub fn next_deadline(&self) -> Option<Instant> {
        [self.writer.next_deadline(), self.reader.next_deadline()]
            .into_iter()
            .flatten()
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Locator;
    use std::time::Duration;

    fn prefix(n: u8) -> GuidPrefix {
        GuidPrefix([n; 12])
    }

    fn participant(n: u8) -> ParticipantData {
        let mut p = ParticipantData::new(prefix(n), VendorId::UNKNOWN);
        p.available_builtin_endpoints.0 |= Wlp::BUILTIN_ENDPOINTS;
        p.metatraffic_unicast_locators = vec![Locator::udpv4(std::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            7400 + n as u16,
        ))];
        p
    }

    fn wlp(n: u8) -> Wlp {
        Wlp::new(
            prefix(n),
            VendorId::UNKNOWN,
            WriterConfig::default(),
            ReaderConfig::default(),
        )
    }

    #[test]
    fn payload_round_trip() {
        let m = ParticipantMessage {
            participant: prefix(3),
            kind: kind::MANUAL_LIVELINESS_UPDATE,
            data: vec![1, 2, 3],
        };
        for e in [Endianness::Big, Endianness::Little] {
            let b = m.to_payload(e);
            // Header, prefix, kind, length, data.
            assert_eq!(b.len(), 4 + 12 + 4 + 4 + 3);
            assert_eq!(ParticipantMessage::from_payload(&b).unwrap(), m);
        }
        let b = m.to_payload(Endianness::Big);
        assert_eq!(&b[..4], &[0, 0, 0, 0]);
        assert_eq!(&b[16..24], &[0, 0, 0, 2, 0, 0, 0, 3]);
        assert!(ParticipantMessage::from_payload(&b[..20]).is_err());
        // As Fast DDS 2.11.2 sends it.
        let mut fast = m.to_payload(Endianness::Little);
        fast[1] = 0x03;
        assert_eq!(ParticipantMessage::from_payload(&fast).unwrap(), m);
        assert_eq!(&m.key_hash()[12..], &[0, 0, 0, 2]);
    }

    /// Delivers everything `from` has due to `to`, until both are quiet.
    fn pump(
        a: &mut Wlp,
        b: &mut Wlp,
        t: Instant,
    ) -> (Vec<LivelinessAsserted>, Vec<LivelinessAsserted>) {
        let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
        for _ in 0..10 {
            let out_a = a.poll(t);
            let out_b = b.poll(t);
            if out_a.is_empty() && out_b.is_empty() {
                break;
            }
            for o in out_a {
                got_b.extend(b.handle_message(&o.bytes, t));
            }
            for o in out_b {
                got_a.extend(a.handle_message(&o.bytes, t));
            }
        }
        (got_a, got_b)
    }

    #[test]
    fn assertions_reach_the_other_participant() {
        let t = Instant::now();
        let mut a = wlp(1);
        let mut b = wlp(2);
        // Written before the other side is known: it is kept for it.
        a.assert_liveliness(kind::AUTOMATIC_LIVELINESS_UPDATE);
        a.add_participant(&participant(2), t);
        b.add_participant(&participant(1), t);
        let (_, got) = pump(&mut a, &mut b, t);
        assert_eq!(
            got,
            [LivelinessAsserted {
                participant: prefix(1),
                kind: kind::AUTOMATIC_LIVELINESS_UPDATE,
            }]
        );
        a.assert_liveliness(kind::MANUAL_LIVELINESS_UPDATE);
        a.assert_liveliness(kind::AUTOMATIC_LIVELINESS_UPDATE);
        let (_, got) = pump(&mut a, &mut b, t + Duration::from_millis(10));
        let kinds: Vec<[u8; 4]> = got.iter().map(|g| g.kind).collect();
        assert_eq!(
            kinds,
            [
                kind::MANUAL_LIVELINESS_UPDATE,
                kind::AUTOMATIC_LIVELINESS_UPDATE
            ]
        );
        // KEEP_LAST 1 of each kind.
        assert_eq!(a.writer.len(), 2);
        // Gone: no more messages to it.
        a.remove_participant(&prefix(2));
        a.assert_liveliness(kind::AUTOMATIC_LIVELINESS_UPDATE);
        assert!(a.poll(t + Duration::from_millis(20)).is_empty());
    }

    #[test]
    fn best_effort_readers_are_served_once() {
        let t = Instant::now();
        let mut a = wlp(1);
        let mut p = participant(2);
        // A reader only, served best-effort.
        p.available_builtin_endpoints =
            BuiltinEndpointSet(BuiltinEndpointSet::PARTICIPANT_MESSAGE_DATA_READER);
        p.builtin_endpoint_qos = Some(BEST_EFFORT_PARTICIPANT_MESSAGE_DATA_READER);
        a.add_participant(&p, t);
        a.assert_liveliness(kind::AUTOMATIC_LIVELINESS_UPDATE);
        assert_eq!(a.poll(t).len(), 1);
        // No HEARTBEAT and no repair for a best-effort reader.
        assert!(a.poll(t + Duration::from_secs(5)).is_empty());
    }
}
