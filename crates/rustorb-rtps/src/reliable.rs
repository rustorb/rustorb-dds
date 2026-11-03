//! Reliable writers and readers (DDSI-RTPS 2.5, 8.4.2, 8.4.9.2 and
//! 8.4.12.2), without I/O.
//!
//! [`ReliableWriter`] keeps a history of changes and, for each matched
//! reader, what was sent, acknowledged and asked for again. It pushes new
//! changes, answers ACKNACKs with DATA or GAP, and sends HEARTBEATs while
//! changes are not acknowledged. [`ReliableReader`] keeps, for each
//! matched writer, which sequence numbers it has; it delivers changes in
//! order and answers HEARTBEATs with ACKNACKs.
//!
//! Both follow the stateful reference implementation in what they send,
//! but are written against the requirements of 8.4.2 only. They are used
//! by SEDP (rustorb-dds#6) and are meant for user endpoints too (D2).
//!
//! The caller passes in received submessages and the time, calls `poll`
//! to get the datagrams to send, and waits until `next_deadline`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use crate::message::{Header, Message};
use crate::parameter::ParameterList;
use crate::submessage::{
    AckNack, Data, Gap, Heartbeat, InfoDestination, SerializedPayload, Submessage,
};
use crate::types::{
    Count, EntityId, Guid, GuidPrefix, Locator, SequenceNumber, SequenceNumberSet, Time, VendorId,
};
use crate::wire::Endianness;

/// A datagram to send to some locators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The encoded RTPS message.
    pub bytes: Vec<u8>,
    /// Where to send it: every locator in the list.
    pub locators: Vec<Locator>,
}

/// A change in a writer's history: what a DATA submessage carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The writer that made the change.
    pub writer: Guid,
    /// Its sequence number.
    pub sequence_number: SequenceNumber,
    /// In-line QoS, such as `PID_KEY_HASH` and `PID_STATUS_INFO`.
    pub inline_qos: Option<ParameterList>,
    /// The serialized value or key.
    pub payload: Option<SerializedPayload>,
    /// The source timestamp (INFO_TS), when received.
    pub timestamp: Option<Time>,
}

/// Where to send to a remote endpoint: its unicast locators, or its
/// multicast ones if it has no unicast locator.
fn destination(unicast: &[Locator], multicast: &[Locator]) -> Vec<Locator> {
    if unicast.is_empty() {
        multicast.to_vec()
    } else {
        unicast.to_vec()
    }
}

/// Puts submessages for one destination into messages of at most
/// `max_size` octets (a single submessage larger than that still goes,
/// alone). Each message starts with INFO_DST.
struct Batcher {
    header: Header,
    dest: GuidPrefix,
    max_size: usize,
    messages: Vec<Message>,
    size: usize,
}

impl Batcher {
    fn new(header: Header, dest: GuidPrefix, max_size: usize) -> Self {
        Batcher {
            header,
            dest,
            max_size,
            messages: Vec::new(),
            size: 0,
        }
    }

    fn push(&mut self, sub: Submessage, approx_size: usize) {
        if self.messages.is_empty() || self.size + approx_size > self.max_size {
            let mut m = Message::new(self.header);
            m.submessages
                .push(Submessage::InfoDestination(InfoDestination {
                    guid_prefix: self.dest,
                }));
            self.messages.push(m);
            self.size = Header::SIZE + 16;
        }
        self.size += approx_size;
        self.messages
            .last_mut()
            .expect("pushed above")
            .submessages
            .push(sub);
    }

    fn finish(self, locators: &[Locator], out: &mut Vec<Outgoing>) {
        for m in self.messages {
            let bytes = m
                .encode(Endianness::NATIVE)
                .expect("messages built here are encodable");
            out.push(Outgoing {
                bytes,
                locators: locators.to_vec(),
            });
        }
    }
}

/// GAP submessages saying that the sequence numbers in `sns` (sorted) are
/// not available: one GAP per run of consecutive numbers.
fn gaps(reader_id: EntityId, writer_id: EntityId, sns: &[SequenceNumber]) -> Vec<Submessage> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < sns.len() {
        let start = sns[i];
        let mut end = start;
        while i + 1 < sns.len() && sns[i + 1].0 == end.0 + 1 {
            i += 1;
            end = sns[i];
        }
        out.push(gap(reader_id, writer_id, start, end));
        i += 1;
    }
    out
}

/// A GAP for `start ..= end`: `gapStart = start`, `gapList.base = end + 1`
/// and an empty bitmap (8.3.8.5).
fn gap(
    reader_id: EntityId,
    writer_id: EntityId,
    start: SequenceNumber,
    end: SequenceNumber,
) -> Submessage {
    Submessage::Gap(Gap {
        reader_id,
        writer_id,
        gap_start: start,
        gap_list: SequenceNumberSet::new(SequenceNumber(end.0 + 1), 0)
            .expect("end + 1 is at least 1"),
        group_info: None,
        filtered_count: None,
    })
}

fn approx_data_size(c: &Change) -> usize {
    // Submessage header, ids, sequence number, payload and some in-line
    // QoS.
    24 + c.payload.as_ref().map_or(0, |p| p.bytes.len())
        + c.inline_qos
            .as_ref()
            .map_or(0, |q| q.parameters.iter().map(|p| 4 + p.value.len()).sum())
}

/// Timing of a [`ReliableWriter`] (8.4.7.1). All of them can be tuned, as
/// 8.4.2.1 requires.
#[derive(Clone, Debug)]
pub struct WriterConfig {
    /// How often to send a HEARTBEAT to readers that have not acknowledged
    /// everything.
    pub heartbeat_period: Duration,
    /// How long to wait after an ACKNACK before sending what it asks for.
    pub nack_response_delay: Duration,
    /// The largest message to build, in octets.
    pub max_message_size: usize,
}

impl Default for WriterConfig {
    fn default() -> Self {
        WriterConfig {
            heartbeat_period: Duration::from_secs(1),
            nack_response_delay: Duration::ZERO,
            max_message_size: 16 * 1024,
        }
    }
}

#[derive(Debug)]
struct ReaderProxy {
    unicast: Vec<Locator>,
    multicast: Vec<Locator>,
    /// Every sequence number up to this one has been sent (as DATA or GAP).
    highest_sent: SequenceNumber,
    /// Every sequence number up to this one has been acknowledged.
    acked: SequenceNumber,
    /// Sequence numbers the reader asked for again.
    requested: BTreeSet<SequenceNumber>,
    repair_at: Option<Instant>,
    last_acknack: Option<Count>,
    /// The reader asked for a HEARTBEAT (ACKNACK without the final flag).
    heartbeat_due: bool,
}

/// A reliable writer: the history and the reader proxies of a stateful
/// writer (8.4.7, 8.4.9.2).
#[derive(Debug)]
pub struct ReliableWriter {
    guid: Guid,
    vendor_id: VendorId,
    config: WriterConfig,
    history: BTreeMap<SequenceNumber, Change>,
    last_sn: SequenceNumber,
    readers: HashMap<Guid, ReaderProxy>,
    heartbeat_count: i32,
    next_heartbeat: Option<Instant>,
}

impl ReliableWriter {
    /// A writer with an empty history and no matched readers.
    pub fn new(guid: Guid, vendor_id: VendorId, config: WriterConfig) -> Self {
        ReliableWriter {
            guid,
            vendor_id,
            config,
            history: BTreeMap::new(),
            last_sn: SequenceNumber(0),
            readers: HashMap::new(),
            heartbeat_count: 0,
            next_heartbeat: None,
        }
    }

    /// The writer's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    /// Adds a change to the history and returns its sequence number. It
    /// is sent to every matched reader at the next [`Self::poll`].
    pub fn add_change(
        &mut self,
        inline_qos: Option<ParameterList>,
        payload: Option<SerializedPayload>,
    ) -> SequenceNumber {
        self.last_sn.0 += 1;
        let sn = self.last_sn;
        self.history.insert(
            sn,
            Change {
                writer: self.guid,
                sequence_number: sn,
                inline_qos,
                payload,
                timestamp: None,
            },
        );
        sn
    }

    /// Removes a change from the history. Readers that have not got it
    /// yet are sent a GAP instead.
    pub fn remove_change(&mut self, sn: SequenceNumber) -> bool {
        self.history.remove(&sn).is_some()
    }

    /// The changes in the history, in order.
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.history.values()
    }

    /// Matches a reader (`matched_reader_add`). It is sent the whole
    /// history. Matching a reader again only updates its locators.
    pub fn add_reader(&mut self, guid: Guid, unicast: Vec<Locator>, multicast: Vec<Locator>) {
        let p = self.readers.entry(guid).or_insert(ReaderProxy {
            unicast: Vec::new(),
            multicast: Vec::new(),
            highest_sent: SequenceNumber(0),
            acked: SequenceNumber(0),
            requested: BTreeSet::new(),
            repair_at: None,
            last_acknack: None,
            heartbeat_due: false,
        });
        p.unicast = unicast;
        p.multicast = multicast;
    }

    /// Unmatches a reader (`matched_reader_remove`).
    pub fn remove_reader(&mut self, guid: &Guid) -> bool {
        self.readers.remove(guid).is_some()
    }

    /// Unmatches every reader of a participant.
    pub fn remove_readers_of(&mut self, prefix: &GuidPrefix) {
        self.readers.retain(|g, _| g.prefix != *prefix);
    }

    /// The matched readers.
    pub fn readers(&self) -> impl Iterator<Item = &Guid> {
        self.readers.keys()
    }

    /// Whether every matched reader has acknowledged `sn`.
    pub fn is_acked_by_all(&self, sn: SequenceNumber) -> bool {
        self.readers.values().all(|r| r.acked >= sn)
    }

    /// Handles an ACKNACK from `reader` (8.4.9.2.8). Duplicates (a count
    /// not above the last one) and ACKNACKs from unmatched readers are
    /// ignored.
    pub fn handle_acknack(&mut self, reader: Guid, a: &AckNack, now: Instant) {
        let Some(p) = self.readers.get_mut(&reader) else {
            return;
        };
        if p.last_acknack.is_some_and(|c| a.count.0 <= c.0) {
            return;
        }
        p.last_acknack = Some(a.count);
        let acked = SequenceNumber(a.reader_sn_state.base().0 - 1);
        // Acknowledging more than was written is not meaningful; cap it.
        p.acked = p.acked.max(acked.min(self.last_sn));
        let mut asked = false;
        for sn in a.reader_sn_state.iter() {
            if sn <= self.last_sn && sn > p.acked {
                p.requested.insert(sn);
                asked = true;
            }
        }
        p.requested.retain(|sn| *sn > p.acked);
        if asked && p.repair_at.is_none() {
            p.repair_at = Some(now + self.config.nack_response_delay);
        }
        if !a.final_flag {
            p.heartbeat_due = true;
        }
    }

    /// Builds what is due at `now`: new changes (T4), repairs (T12) and
    /// HEARTBEATs (T7).
    pub fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        let periodic = self.next_heartbeat.is_none_or(|t| t <= now);
        if periodic {
            self.next_heartbeat = Some(now + self.config.heartbeat_period);
        }
        let header = Header::new(self.vendor_id, self.guid.prefix);
        let writer_id = self.guid.entity_id;
        let first_sn = self
            .history
            .keys()
            .next()
            .copied()
            .unwrap_or(SequenceNumber(self.last_sn.0 + 1));
        let mut out = Vec::new();
        let mut guids: Vec<Guid> = self.readers.keys().copied().collect();
        guids.sort();
        for g in guids {
            let mut subs = Vec::new();
            let mut sent_something = false;
            let p = self.readers.get_mut(&g).expect("listed above");
            // Repairs.
            if p.repair_at.is_some_and(|t| t <= now) {
                p.repair_at = None;
                let mut missing = Vec::new();
                for sn in std::mem::take(&mut p.requested) {
                    match self.history.get(&sn) {
                        Some(c) => subs.push((data(g.entity_id, c), approx_data_size(c))),
                        None => missing.push(sn),
                    }
                }
                for s in gaps(g.entity_id, writer_id, &missing) {
                    subs.push((s, 32));
                }
                sent_something = true;
            }
            // New changes.
            if p.highest_sent < self.last_sn {
                let mut next = SequenceNumber(p.highest_sent.0 + 1);
                for (sn, c) in self.history.range(next..) {
                    if *sn > next {
                        subs.push((
                            gap(g.entity_id, writer_id, next, SequenceNumber(sn.0 - 1)),
                            32,
                        ));
                    }
                    subs.push((data(g.entity_id, c), approx_data_size(c)));
                    next = SequenceNumber(sn.0 + 1);
                }
                if next <= self.last_sn {
                    subs.push((gap(g.entity_id, writer_id, next, self.last_sn), 32));
                }
                p.highest_sent = self.last_sn;
                sent_something = true;
            }
            let unacked = p.acked < self.last_sn;
            let want_hb = p.heartbeat_due || sent_something || (periodic && unacked);
            p.heartbeat_due = false;
            let dest = destination(&p.unicast, &p.multicast);
            if want_hb {
                // A HEARTBEAT after data asks the reader to answer, so it
                // can ask for what it missed (final flag not set).
                self.heartbeat_count = self.heartbeat_count.wrapping_add(1);
                let hb = Heartbeat {
                    final_flag: !unacked,
                    liveliness_flag: false,
                    reader_id: g.entity_id,
                    writer_id,
                    first_sn,
                    last_sn: self.last_sn,
                    count: Count(self.heartbeat_count),
                    group_info: None,
                };
                subs.push((Submessage::Heartbeat(hb), 32));
            }
            if subs.is_empty() {
                continue;
            }
            let mut b = Batcher::new(header, g.prefix, self.config.max_message_size);
            for (s, size) in subs {
                b.push(s, size);
            }
            b.finish(&dest, &mut out);
        }
        out
    }

    /// When [`Self::poll`] next has something to do, if no message comes
    /// in before.
    pub fn next_deadline(&self) -> Option<Instant> {
        let mut d = None::<Instant>;
        let mut take = |t: Instant| d = Some(d.map_or(t, |d| d.min(t)));
        for p in self.readers.values() {
            if let Some(t) = p.repair_at {
                take(t);
            }
            if p.highest_sent < self.last_sn || p.heartbeat_due {
                take(Instant::now());
            }
        }
        if self.readers.values().any(|p| p.acked < self.last_sn) {
            if let Some(t) = self.next_heartbeat {
                take(t);
            }
        }
        d
    }
}

fn data(reader_id: EntityId, c: &Change) -> Submessage {
    Submessage::Data(Data {
        reader_id,
        writer_id: c.writer.entity_id,
        writer_sn: c.sequence_number,
        inline_qos: c.inline_qos.clone(),
        serialized_payload: c.payload.clone(),
        non_standard_payload: false,
    })
}

/// Timing of a [`ReliableReader`] (8.4.10.1).
#[derive(Clone, Debug)]
pub struct ReaderConfig {
    /// How long to wait after a HEARTBEAT before answering it.
    pub heartbeat_response_delay: Duration,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        ReaderConfig {
            heartbeat_response_delay: Duration::ZERO,
        }
    }
}

#[derive(Debug)]
enum Pending {
    Change(Change),
    /// Sequence numbers up to and including this one will not come.
    Irrelevant(SequenceNumber),
}

#[derive(Debug)]
struct WriterProxy {
    unicast: Vec<Locator>,
    multicast: Vec<Locator>,
    /// Every sequence number below this one was delivered or will not
    /// come.
    next_expected: SequenceNumber,
    /// What came at or after `next_expected`, out of order.
    pending: BTreeMap<SequenceNumber, Pending>,
    /// The highest sequence number the writer said it has.
    available_max: SequenceNumber,
    last_heartbeat: Option<Count>,
    ack_at: Option<Instant>,
}

impl WriterProxy {
    /// Takes, in order, what can be delivered.
    fn drain(&mut self, out: &mut Vec<Change>) {
        while let Some((&k, _)) = self.pending.first_key_value() {
            if k > self.next_expected {
                break;
            }
            let (_, e) = self.pending.pop_first().expect("looked at above");
            match e {
                Pending::Change(c) => {
                    if k == self.next_expected {
                        out.push(c);
                        self.next_expected.0 += 1;
                    }
                }
                Pending::Irrelevant(end) => {
                    if end >= self.next_expected {
                        self.next_expected = SequenceNumber(end.0 + 1);
                    }
                }
            }
        }
    }

    fn mark_irrelevant(&mut self, start: SequenceNumber, end: SequenceNumber) {
        let start = start.max(self.next_expected);
        if end < start {
            return;
        }
        match self.pending.get(&start) {
            Some(Pending::Irrelevant(e)) if *e >= end => {}
            _ => {
                self.pending.insert(start, Pending::Irrelevant(end));
            }
        }
    }

    /// Whether `sn` (at or after `next_expected`) is known.
    fn has(&self, sn: SequenceNumber) -> bool {
        self.pending.range(..=sn).any(|(k, e)| match e {
            Pending::Change(_) => *k == sn,
            Pending::Irrelevant(end) => sn <= *end,
        })
    }

    /// Missing sequence numbers, from `next_expected` up to what the
    /// writer has, at most 256 of them.
    fn missing(&self) -> Vec<SequenceNumber> {
        let mut out = Vec::new();
        let mut sn = self.next_expected;
        while sn <= self.available_max && sn.0 < self.next_expected.0 + 256 {
            if !self.has(sn) {
                out.push(sn);
            }
            sn.0 += 1;
        }
        out
    }
}

/// A reliable reader: the writer proxies of a stateful reader (8.4.10,
/// 8.4.12.2). Changes are delivered once each and, for each writer, in
/// sequence number order.
#[derive(Debug)]
pub struct ReliableReader {
    guid: Guid,
    vendor_id: VendorId,
    config: ReaderConfig,
    writers: HashMap<Guid, WriterProxy>,
    acknack_count: i32,
}

impl ReliableReader {
    /// A reader with no matched writers.
    pub fn new(guid: Guid, vendor_id: VendorId, config: ReaderConfig) -> Self {
        ReliableReader {
            guid,
            vendor_id,
            config,
            writers: HashMap::new(),
            acknack_count: 0,
        }
    }

    /// The reader's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    /// Matches a writer (`matched_writer_add`). An ACKNACK is sent to it
    /// at the next [`Self::poll`], so that it learns about this reader
    /// early (8.4.2.3.4 allows this). Matching a writer again only
    /// updates its locators.
    pub fn add_writer(
        &mut self,
        guid: Guid,
        unicast: Vec<Locator>,
        multicast: Vec<Locator>,
        now: Instant,
    ) {
        let p = self.writers.entry(guid).or_insert(WriterProxy {
            unicast: Vec::new(),
            multicast: Vec::new(),
            next_expected: SequenceNumber(1),
            pending: BTreeMap::new(),
            available_max: SequenceNumber(0),
            last_heartbeat: None,
            ack_at: Some(now),
        });
        p.unicast = unicast;
        p.multicast = multicast;
    }

    /// Unmatches a writer (`matched_writer_remove`).
    pub fn remove_writer(&mut self, guid: &Guid) -> bool {
        self.writers.remove(guid).is_some()
    }

    /// Unmatches every writer of a participant.
    pub fn remove_writers_of(&mut self, prefix: &GuidPrefix) {
        self.writers.retain(|g, _| g.prefix != *prefix);
    }

    /// The matched writers.
    pub fn writers(&self) -> impl Iterator<Item = &Guid> {
        self.writers.keys()
    }

    /// Handles a DATA from `writer` (8.4.12.2.8) and returns the changes
    /// that can now be delivered. DATA from unmatched writers and
    /// duplicates are ignored.
    pub fn handle_data(&mut self, writer: Guid, d: &Data, timestamp: Option<Time>) -> Vec<Change> {
        let mut out = Vec::new();
        let Some(p) = self.writers.get_mut(&writer) else {
            return out;
        };
        let sn = d.writer_sn;
        if sn < p.next_expected || p.has(sn) {
            return out;
        }
        p.pending.insert(
            sn,
            Pending::Change(Change {
                writer,
                sequence_number: sn,
                inline_qos: d.inline_qos.clone(),
                payload: d.serialized_payload.clone(),
                timestamp,
            }),
        );
        p.available_max = p.available_max.max(sn);
        p.drain(&mut out);
        out
    }

    /// Handles a GAP from `writer` (8.4.12.2.9) and returns the changes
    /// that can now be delivered.
    pub fn handle_gap(&mut self, writer: Guid, g: &Gap) -> Vec<Change> {
        let mut out = Vec::new();
        let Some(p) = self.writers.get_mut(&writer) else {
            return out;
        };
        let base = g.gap_list.base();
        p.mark_irrelevant(g.gap_start, SequenceNumber(base.0 - 1));
        for sn in g.gap_list.iter() {
            p.mark_irrelevant(sn, sn);
        }
        p.drain(&mut out);
        out
    }

    /// Handles a HEARTBEAT from `writer` (8.4.12.2.7) and returns the
    /// changes that can now be delivered: changes the writer no longer
    /// has are given up on. Duplicates (a count not above the last one)
    /// are ignored.
    pub fn handle_heartbeat(&mut self, writer: Guid, h: &Heartbeat, now: Instant) -> Vec<Change> {
        let mut out = Vec::new();
        let Some(p) = self.writers.get_mut(&writer) else {
            return out;
        };
        if p.last_heartbeat.is_some_and(|c| h.count.0 <= c.0) {
            return out;
        }
        p.last_heartbeat = Some(h.count);
        p.mark_irrelevant(p.next_expected, SequenceNumber(h.first_sn.0 - 1));
        p.available_max = p.available_max.max(h.last_sn);
        p.drain(&mut out);
        let liveliness_only = h.liveliness_flag && h.final_flag;
        if !liveliness_only && (!h.final_flag || !p.missing().is_empty()) && p.ack_at.is_none() {
            p.ack_at = Some(now + self.config.heartbeat_response_delay);
        }
        out
    }

    /// Builds the ACKNACKs due at `now` (8.4.12.2.5).
    pub fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        let header = Header::new(self.vendor_id, self.guid.prefix);
        let mut out = Vec::new();
        let mut guids: Vec<Guid> = self.writers.keys().copied().collect();
        guids.sort();
        for g in guids {
            let p = self.writers.get_mut(&g).expect("listed above");
            if !p.ack_at.is_some_and(|t| t <= now) {
                continue;
            }
            p.ack_at = None;
            let missing = p.missing();
            let base = p.next_expected;
            let num_bits = missing.last().map_or(0, |m| (m.0 - base.0 + 1) as u32);
            let mut set = SequenceNumberSet::new(base, num_bits).expect("base >= 1, <= 256 bits");
            for sn in &missing {
                set.insert(*sn);
            }
            self.acknack_count = self.acknack_count.wrapping_add(1);
            let a = Submessage::AckNack(AckNack {
                final_flag: missing.is_empty(),
                reader_id: self.guid.entity_id,
                writer_id: g.entity_id,
                reader_sn_state: set,
                count: Count(self.acknack_count),
            });
            let mut b = Batcher::new(header, g.prefix, usize::MAX);
            b.push(a, 0);
            b.finish(&destination(&p.unicast, &p.multicast), &mut out);
        }
        out
    }

    /// When [`Self::poll`] next has something to do, if no message comes
    /// in before.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.writers.values().filter_map(|p| p.ack_at).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submessage::PayloadKind;
    use crate::types::entity_kind;

    fn wguid() -> Guid {
        Guid::new(
            GuidPrefix([1; 12]),
            EntityId::user([0, 0, 1], entity_kind::USER_WRITER_WITH_KEY),
        )
    }

    fn rguid(n: u8) -> Guid {
        Guid::new(
            GuidPrefix([n; 12]),
            EntityId::user([0, 0, 2], entity_kind::USER_READER_WITH_KEY),
        )
    }

    fn loc(port: u16) -> Vec<Locator> {
        vec![Locator::udpv4(std::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            port,
        ))]
    }

    fn payload(n: u8) -> Option<SerializedPayload> {
        Some(SerializedPayload {
            kind: PayloadKind::Data,
            bytes: vec![0, 1, 0, 0, n, 0, 0, 0],
        })
    }

    /// The entity submessages of the datagrams.
    fn subs(out: &[Outgoing]) -> Vec<Submessage> {
        out.iter()
            .flat_map(|o| Message::decode(&o.bytes).unwrap().submessages)
            .filter(|s| !matches!(s, Submessage::InfoDestination(_)))
            .collect()
    }

    /// Feeds the writer's datagrams to the reader, dropping those whose
    /// index is in `drop`; returns the delivered changes.
    fn deliver(
        r: &mut ReliableReader,
        out: &[Outgoing],
        drop: &[usize],
        now: Instant,
    ) -> Vec<Change> {
        let mut got = Vec::new();
        let mut i = 0;
        for o in out {
            let m = Message::decode(&o.bytes).unwrap();
            for s in m.submessages {
                let w = Guid::new(m.header.guid_prefix, wguid().entity_id);
                match s {
                    Submessage::Data(d) => {
                        if !drop.contains(&i) {
                            got.extend(r.handle_data(w, &d, None));
                        }
                        i += 1;
                    }
                    Submessage::Gap(g) => got.extend(r.handle_gap(w, &g)),
                    Submessage::Heartbeat(h) => got.extend(r.handle_heartbeat(w, &h, now)),
                    _ => {}
                }
            }
        }
        got
    }

    fn acknacks(w: &mut ReliableWriter, out: &[Outgoing], now: Instant) {
        for o in out {
            let m = Message::decode(&o.bytes).unwrap();
            for s in m.submessages {
                if let Submessage::AckNack(a) = s {
                    w.handle_acknack(Guid::new(m.header.guid_prefix, a.reader_id), &a, now);
                }
            }
        }
    }

    #[test]
    fn push_lose_and_repair() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, WriterConfig::default());
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        w.add_reader(rguid(2), loc(2), vec![]);
        for i in 1..=4 {
            w.add_change(None, payload(i));
        }
        let out = w.poll(t);
        assert_eq!(out[0].locators, loc(2));
        let s = subs(&out);
        assert_eq!(s.len(), 5);
        assert!(matches!(&s[4], Submessage::Heartbeat(h)
            if h.first_sn == SequenceNumber(1) && h.last_sn == SequenceNumber(4) && !h.final_flag));

        // DATA 2 and 3 are lost: only 1 is delivered.
        let got = deliver(&mut r, &out, &[1, 2], t);
        assert_eq!(got.len(), 1);
        let acks = r.poll(t);
        assert_eq!(acks[0].locators, loc(1));
        let Submessage::AckNack(a) = &subs(&acks)[0] else {
            panic!()
        };
        assert_eq!(a.reader_sn_state.base(), SequenceNumber(2));
        assert_eq!(
            a.reader_sn_state.iter().collect::<Vec<_>>(),
            [SequenceNumber(2), SequenceNumber(3)]
        );
        assert!(!a.final_flag);
        acknacks(&mut w, &acks, t);
        assert!(w.is_acked_by_all(SequenceNumber(1)));
        assert!(!w.is_acked_by_all(SequenceNumber(2)));

        let out = w.poll(t);
        let s = subs(&out);
        assert!(matches!(&s[0], Submessage::Data(d) if d.writer_sn == SequenceNumber(2)));
        assert!(matches!(&s[1], Submessage::Data(d) if d.writer_sn == SequenceNumber(3)));
        let got = deliver(&mut r, &out, &[], t);
        let sns: Vec<i64> = got.iter().map(|c| c.sequence_number.0).collect();
        assert_eq!(sns, [2, 3, 4]);
        let acks = r.poll(t);
        let Submessage::AckNack(a) = &subs(&acks)[0] else {
            panic!()
        };
        assert_eq!(a.reader_sn_state.base(), SequenceNumber(5));
        assert!(a.final_flag);
        acknacks(&mut w, &acks, t);
        assert!(w.is_acked_by_all(SequenceNumber(4)));
        // Everything acknowledged: no periodic HEARTBEAT.
        assert!(w.poll(t + Duration::from_secs(5)).is_empty());
        assert_eq!(w.next_deadline(), None);
    }

    #[test]
    fn removed_changes_become_gaps() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, WriterConfig::default());
        for i in 1..=5 {
            w.add_change(None, payload(i));
        }
        w.remove_change(SequenceNumber(2));
        w.remove_change(SequenceNumber(3));
        w.remove_change(SequenceNumber(5));
        // A late reader gets the history with GAPs.
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        w.add_reader(rguid(2), loc(2), vec![]);
        let out = w.poll(t);
        let got = deliver(&mut r, &out, &[], t);
        let sns: Vec<i64> = got.iter().map(|c| c.sequence_number.0).collect();
        assert_eq!(sns, [1, 4]);
        let s = subs(&out);
        assert!(matches!(&s[1], Submessage::Gap(g)
            if g.gap_start == SequenceNumber(2) && g.gap_list.base() == SequenceNumber(4)));

        // A NACK for a removed change is answered with a GAP.
        let mut set = SequenceNumberSet::new(SequenceNumber(1), 8).unwrap();
        set.insert(SequenceNumber(3));
        w.handle_acknack(
            rguid(2),
            &AckNack {
                final_flag: false,
                reader_id: rguid(2).entity_id,
                writer_id: wguid().entity_id,
                reader_sn_state: set,
                count: Count(1),
            },
            t,
        );
        let s = subs(&w.poll(t));
        assert!(matches!(&s[0], Submessage::Gap(g) if g.gap_start == SequenceNumber(3)));
        assert!(matches!(&s[1], Submessage::Heartbeat(_)));
    }

    #[test]
    fn heartbeat_gives_up_on_removed_and_counts_dedupe() {
        let t = Instant::now();
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        // The first poll sends the early ACKNACK.
        assert_eq!(r.poll(t).len(), 1);
        assert!(r.poll(t).is_empty());
        let d = |sn| Data {
            reader_id: EntityId::UNKNOWN,
            writer_id: wguid().entity_id,
            writer_sn: SequenceNumber(sn),
            inline_qos: None,
            serialized_payload: payload(sn as u8),
            non_standard_payload: false,
        };
        assert!(r.handle_data(wguid(), &d(3), None).is_empty());
        let hb = |first, last, count, final_flag| Heartbeat {
            final_flag,
            liveliness_flag: false,
            reader_id: EntityId::UNKNOWN,
            writer_id: wguid().entity_id,
            first_sn: SequenceNumber(first),
            last_sn: SequenceNumber(last),
            count: Count(count),
            group_info: None,
        };
        // The writer no longer has 1 and 2: 3 is delivered.
        let got = r.handle_heartbeat(wguid(), &hb(3, 4, 1, true), t);
        assert_eq!(got.len(), 1);
        // 4 is missing, so an ACKNACK is due even with the final flag.
        let Submessage::AckNack(a) = &subs(&r.poll(t))[0] else {
            panic!()
        };
        assert_eq!(a.reader_sn_state.base(), SequenceNumber(4));
        assert!(a.reader_sn_state.contains(SequenceNumber(4)));
        // Same count again: ignored.
        r.handle_heartbeat(wguid(), &hb(3, 4, 1, false), t);
        assert!(r.poll(t).is_empty());
        // Duplicate DATA is dropped.
        assert_eq!(r.handle_data(wguid(), &d(4), None).len(), 1);
        assert!(r.handle_data(wguid(), &d(4), None).is_empty());
        // Nothing missing and final: no answer.
        r.handle_heartbeat(wguid(), &hb(3, 4, 2, true), t);
        assert!(r.poll(t).is_empty());
        // Unmatched writer: ignored.
        assert!(r.handle_data(rguid(9), &d(5), None).is_empty());
    }

    #[test]
    fn periodic_heartbeat_until_acked() {
        let t = Instant::now();
        let cfg = WriterConfig {
            heartbeat_period: Duration::from_millis(100),
            ..WriterConfig::default()
        };
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, cfg);
        w.add_reader(rguid(2), loc(2), loc(3));
        w.add_reader(rguid(3), vec![], loc(3));
        w.add_change(None, payload(1));
        let out = w.poll(t);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].locators, loc(3));
        assert!(w.poll(t + Duration::from_millis(50)).is_empty());
        let out = w.poll(t + Duration::from_millis(100));
        assert_eq!(out.len(), 2);
        assert!(
            subs(&out)
                .iter()
                .all(|s| matches!(s, Submessage::Heartbeat(_)))
        );
        assert_eq!(w.next_deadline(), Some(t + Duration::from_millis(200)));
        w.remove_reader(&rguid(2));
        w.remove_readers_of(&rguid(3).prefix);
        assert_eq!(w.readers().count(), 0);
    }
}
