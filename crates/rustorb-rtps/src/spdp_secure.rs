//! The DCPSParticipantsSecure builtin topic (DDS Security 1.2, 7.5.1.6).
//!
//! A participant with protected discovery announces itself on SPDP as
//! usual, to bootstrap authentication, and once a remote participant is
//! authenticated also sends it its `ParticipantBuiltinTopicDataSecure`
//! through the SPDPbuiltinParticipantsSecureWriter: the SPDP data, with
//! an optional `identity_status_token`. Changes to the data and the
//! deletion of the participant go there too. The endpoints have the QoS
//! of the secure SEDP endpoints: RELIABLE and TRANSIENT_LOCAL, with one
//! instance, the participant, kept KEEP_LAST 1.
//!
//! [`SpdpSecure`] holds the two endpoints without doing any I/O, like
//! [`crate::wlp::Wlp`]; DDS Security protects what they send. Their
//! entity ids are in 7.5.8 and their bits of `availableBuiltinEndpoints`
//! in 7.4.1.6.

use std::time::Instant;

use crate::error::DecodeError;
use crate::parameter::{Parameter, ParameterList};
use crate::pl::{PlBuilder, decode_pl_payload, encode_pl_payload, pid};
use crate::receiver::receive;
use crate::reliable::{
    MatchedReader, Outgoing, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use crate::spdp::ParticipantData;
use crate::submessage::{PayloadKind, SerializedPayload, Submessage};
use crate::types::{BuiltinEndpointSet, EntityId, Guid, GuidPrefix, SequenceNumber, VendorId};
use crate::wire::Endianness;

/// `StatusInfo_t` flags (DDSI-RTPS 2.5, 9.6.4.9).
const STATUS_DISPOSED: u8 = 0x01;
const STATUS_UNREGISTERED: u8 = 0x02;

/// What a remote participant sent on DCPSParticipantsSecure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureParticipantEvent {
    /// Its current data.
    Data(Box<ParticipantData>),
    /// It is going away (disposed or unregistered).
    Disposed(GuidPrefix),
}

/// The SPDPbuiltinParticipantsSecureWriter and Reader of one participant.
#[derive(Debug)]
pub struct SpdpSecure {
    prefix: GuidPrefix,
    writer: ReliableWriter,
    reader: ReliableReader,
    /// The change with the local data (KEEP_LAST 1).
    last: Option<SequenceNumber>,
}

impl SpdpSecure {
    /// The bits of `availableBuiltinEndpoints` for the endpoints provided
    /// here.
    pub const BUILTIN_ENDPOINTS: u32 = BuiltinEndpointSet::PARTICIPANT_SECURE_WRITER
        | BuiltinEndpointSet::PARTICIPANT_SECURE_READER;

    /// The endpoints of the participant `prefix`.
    pub fn new(
        prefix: GuidPrefix,
        vendor_id: VendorId,
        writer: WriterConfig,
        reader: ReaderConfig,
    ) -> Self {
        SpdpSecure {
            prefix,
            writer: ReliableWriter::new(
                Guid::new(
                    prefix,
                    EntityId::SPDP_RELIABLE_BUILTIN_PARTICIPANT_SECURE_WRITER,
                ),
                vendor_id,
                writer,
            ),
            reader: ReliableReader::new(
                Guid::new(
                    prefix,
                    EntityId::SPDP_RELIABLE_BUILTIN_PARTICIPANT_SECURE_READER,
                ),
                vendor_id,
                reader,
            ),
            last: None,
        }
    }

    /// Connects with the endpoints of an authenticated participant, as
    /// far as its `availableBuiltinEndpoints` says it has them. The local
    /// participant is ignored.
    pub fn add_participant(&mut self, p: &ParticipantData, now: Instant) {
        if p.guid_prefix == self.prefix {
            return;
        }
        let set = p.available_builtin_endpoints;
        let uc = p.metatraffic_unicast_locators.clone();
        let mc = p.metatraffic_multicast_locators.clone();
        if set.contains(BuiltinEndpointSet::PARTICIPANT_SECURE_READER) {
            self.writer.add_matched_reader(
                Guid::new(
                    p.guid_prefix,
                    EntityId::SPDP_RELIABLE_BUILTIN_PARTICIPANT_SECURE_READER,
                ),
                MatchedReader {
                    unicast: uc.clone(),
                    multicast: mc.clone(),
                    reliable: true,
                    history: true,
                },
            );
        }
        if set.contains(BuiltinEndpointSet::PARTICIPANT_SECURE_WRITER) {
            self.reader.add_writer(
                Guid::new(
                    p.guid_prefix,
                    EntityId::SPDP_RELIABLE_BUILTIN_PARTICIPANT_SECURE_WRITER,
                ),
                uc,
                mc,
                now,
            );
        }
    }

    /// Disconnects from a participant that is gone.
    pub fn remove_participant(&mut self, prefix: &GuidPrefix) {
        self.writer.remove_readers_of(prefix);
        self.reader.remove_writers_of(prefix);
    }

    /// Writes the local participant's data, replacing what was written
    /// before. It is sent at the next [`Self::poll`].
    pub fn write(&mut self, data: &ParticipantData) {
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, data.guid().to_bytes()));
        let sn = self.writer.add_change(
            Some(qos),
            Some(SerializedPayload {
                kind: PayloadKind::Data,
                bytes: data.to_payload(Endianness::NATIVE),
            }),
        );
        if let Some(old) = self.last.replace(sn) {
            self.writer.remove_change(old);
        }
    }

    /// Writes that the local participant is going away: disposed and
    /// unregistered, with the key, `PID_PARTICIPANT_GUID`, as the
    /// payload, as on SPDP (DDSI-RTPS 2.5, 9.6.3.2). Returns its sequence
    /// number, for [`Self::is_acked_by_all`].
    pub fn dispose(&mut self) -> SequenceNumber {
        let e = Endianness::NATIVE;
        let guid = Guid::new(self.prefix, EntityId::PARTICIPANT);
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, guid.to_bytes()));
        qos.push(Parameter::new(
            pid::STATUS_INFO,
            [0, 0, 0, STATUS_DISPOSED | STATUS_UNREGISTERED],
        ));
        let mut key = ParameterList::new();
        key.push(PlBuilder::new(e).guid(pid::PARTICIPANT_GUID, guid));
        let sn = self.writer.add_change(
            Some(qos),
            Some(SerializedPayload {
                kind: PayloadKind::Key,
                bytes: encode_pl_payload(&key, e),
            }),
        );
        if let Some(old) = self.last.replace(sn) {
            self.writer.remove_change(old);
        }
        sn
    }

    /// Whether every matched reader acknowledged the change `sn`.
    pub fn is_acked_by_all(&self, sn: SequenceNumber) -> bool {
        self.writer.is_acked_by_all(sn)
    }

    /// Processes one received RTPS message, decoded by DDS Security:
    /// returns what other participants sent. The data and disposes are
    /// taken as the writer's participant's own, whatever they say.
    pub fn handle_message(&mut self, bytes: &[u8], now: Instant) -> Vec<SecureParticipantEvent> {
        let mut out = Vec::new();
        let Some((_, received)) = receive(bytes, self.prefix) else {
            return out;
        };
        let w = EntityId::SPDP_RELIABLE_BUILTIN_PARTICIPANT_SECURE_WRITER;
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
                let from = c.writer.prefix;
                let status = c
                    .inline_qos
                    .as_ref()
                    .and_then(|q| q.get(pid::STATUS_INFO))
                    .and_then(|p| p.value.get(3).copied())
                    .unwrap_or(0);
                if status & (STATUS_DISPOSED | STATUS_UNREGISTERED) != 0 {
                    out.push(SecureParticipantEvent::Disposed(from));
                    continue;
                }
                let Some(p) = c.payload.filter(|p| p.kind == PayloadKind::Data) else {
                    continue;
                };
                if let Ok(d) = Self::data_from_payload(&p.bytes) {
                    if d.guid_prefix == from {
                        out.push(SecureParticipantEvent::Data(Box::new(d)));
                    }
                }
            }
        }
        out
    }

    /// Reads `ParticipantBuiltinTopicDataSecure`: the SPDP data, the
    /// `identity_status_token` being skipped.
    fn data_from_payload(bytes: &[u8]) -> Result<ParticipantData, DecodeError> {
        let (mut list, e) = decode_pl_payload(bytes)?;
        list.parameters
            .retain(|p| p.id != pid::IDENTITY_STATUS_TOKEN);
        ParticipantData::from_parameter_list(&list, e)
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
        p.available_builtin_endpoints.0 |= SpdpSecure::BUILTIN_ENDPOINTS;
        p.metatraffic_unicast_locators = vec![Locator::udpv4(std::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            7400 + n as u16,
        ))];
        p
    }

    fn secure(n: u8) -> SpdpSecure {
        SpdpSecure::new(
            prefix(n),
            VendorId::UNKNOWN,
            WriterConfig::default(),
            ReaderConfig::default(),
        )
    }

    fn pump(
        a: &mut SpdpSecure,
        b: &mut SpdpSecure,
        t: Instant,
    ) -> (Vec<SecureParticipantEvent>, Vec<SecureParticipantEvent>) {
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
    fn data_changes_and_deletion_reach_the_other_participant() {
        let t = Instant::now();
        let mut a = secure(1);
        let mut b = secure(2);
        let mut data = participant(1);
        // Written before the other side is known: it is kept for it.
        a.write(&data);
        a.add_participant(&participant(2), t);
        b.add_participant(&data, t);
        let (_, got) = pump(&mut a, &mut b, t);
        assert_eq!(got, [SecureParticipantEvent::Data(Box::new(data.clone()))]);
        // A change replaces the data (KEEP_LAST 1).
        data.user_data = vec![1, 2, 3];
        a.write(&data);
        assert_eq!(a.writer.len(), 1);
        let (_, got) = pump(&mut a, &mut b, t + Duration::from_millis(10));
        assert_eq!(got, [SecureParticipantEvent::Data(Box::new(data.clone()))]);
        let sn = a.dispose();
        let (_, got) = pump(&mut a, &mut b, t + Duration::from_millis(20));
        assert_eq!(got, [SecureParticipantEvent::Disposed(prefix(1))]);
        assert!(a.is_acked_by_all(sn));
    }

    #[test]
    fn data_of_another_participant_is_ignored() {
        let t = Instant::now();
        let mut a = secure(1);
        let mut b = secure(2);
        // Participant 1 writes data that claims to be participant 3's.
        a.write(&participant(3));
        a.add_participant(&participant(2), t);
        b.add_participant(&participant(1), t);
        let (_, got) = pump(&mut a, &mut b, t);
        assert!(got.is_empty());
    }

    #[test]
    fn the_identity_status_token_is_skipped() {
        let d = participant(4);
        let e = Endianness::Little;
        let mut list = d.to_parameter_list(e);
        list.push(Parameter::new(pid::IDENTITY_STATUS_TOKEN, vec![0u8; 8]));
        let bytes = encode_pl_payload(&list, e);
        assert_eq!(SpdpSecure::data_from_payload(&bytes).unwrap(), d);
    }
}
