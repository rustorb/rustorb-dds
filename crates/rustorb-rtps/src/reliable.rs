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
//! by SEDP (rustorb-dds#6) and by the user endpoints of rustorb-dds
//! (rustorb-dds#16).
//!
//! A [`ReliableWriter`] can also serve best-effort readers (8.4.4: a
//! RELIABLE writer matches BEST_EFFORT readers too): they are sent each
//! change once, and no HEARTBEAT, GAP or repair.
//!
//! Changes too large for a datagram go as DATA_FRAG (8.4.14.1, see
//! [`crate::frag`]): the writer resends the fragments a NACK_FRAG asks
//! for, and the reader asks with NACK_FRAG for the fragments it misses
//! of a change it has part of.
//!
//! The caller passes in received submessages and the time, calls `poll`
//! to get the datagrams to send, and waits until `next_deadline`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use crate::frag::{self, Fragmentation, Keep, MESSAGE_OVERHEAD, Reassembler, ReassemblyLimits};
use crate::message::{Header, Message};
use crate::parameter::ParameterList;
use crate::submessage::{
    AckNack, Data, DataFrag, Gap, Heartbeat, HeartbeatFrag, InfoDestination, InfoTimestamp,
    NackFrag, SerializedPayload, Submessage,
};
use crate::types::{
    Count, EntityId, FragmentNumber, FragmentNumberSet, Guid, GuidPrefix, Locator, SequenceNumber,
    SequenceNumberSet, Time, VendorId,
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
    /// The source timestamp in force in the last message (8.3.4: INFO_TS
    /// applies to the submessages after it in the same message).
    timestamp: Option<Time>,
}

impl Batcher {
    fn new(header: Header, dest: GuidPrefix, max_size: usize) -> Self {
        Batcher {
            header,
            dest,
            max_size,
            messages: Vec::new(),
            size: 0,
            timestamp: None,
        }
    }

    /// Pushes a change: a DATA, or DATA_FRAGs when it is too large
    /// (only the fragments in `fragments`, when given).
    fn push_change(
        &mut self,
        reader_id: EntityId,
        c: &Change,
        frag: &Fragmentation,
        fragments: Option<&BTreeSet<u32>>,
    ) {
        if !frag.needed(c) {
            self.push_timestamped(c.timestamp, data(reader_id, c), frag::data_len(c));
            return;
        }
        for d in frag.data_frags(reader_id, c, fragments, self.max_size) {
            let size = 36
                + frag::inline_qos_len(d.inline_qos.as_ref())
                + d.serialized_payload.len().next_multiple_of(4);
            self.push_timestamped(c.timestamp, Submessage::DataFrag(d), size);
        }
    }

    /// Pushes a DATA or DATA_FRAG, preceded by an INFO_TS when its
    /// timestamp differs from the one in force.
    fn push_timestamped(&mut self, timestamp: Option<Time>, sub: Submessage, size: usize) {
        self.start_if_full(size + 12);
        if self.timestamp != timestamp {
            self.timestamp = timestamp;
            self.push_unchecked(Submessage::InfoTimestamp(InfoTimestamp { timestamp }), 12);
        }
        self.push_unchecked(sub, size);
    }

    fn push(&mut self, sub: Submessage, approx_size: usize) {
        self.start_if_full(approx_size);
        self.push_unchecked(sub, approx_size);
    }

    fn start_if_full(&mut self, approx_size: usize) {
        if self.messages.is_empty() || self.size + approx_size > self.max_size {
            let mut m = Message::new(self.header);
            m.submessages
                .push(Submessage::InfoDestination(InfoDestination {
                    guid_prefix: self.dest,
                }));
            self.messages.push(m);
            self.size = MESSAGE_OVERHEAD - 12;
            self.timestamp = None;
        }
    }

    fn push_unchecked(&mut self, sub: Submessage, approx_size: usize) {
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

/// Timing of a [`ReliableWriter`] (8.4.7.1). All of them can be tuned, as
/// 8.4.2.1 requires.
#[derive(Clone, Debug)]
pub struct WriterConfig {
    /// How often to send a HEARTBEAT to readers that have not acknowledged
    /// everything.
    pub heartbeat_period: Duration,
    /// How long to wait after an ACKNACK before sending what it asks for.
    pub nack_response_delay: Duration,
    /// The largest message to build, in octets. A change too large for
    /// it still goes alone in a message, unless it has to be fragmented.
    pub max_message_size: usize,
    /// When and how to send a change as DATA_FRAG.
    pub fragmentation: Fragmentation,
}

impl Default for WriterConfig {
    fn default() -> Self {
        WriterConfig {
            heartbeat_period: Duration::from_secs(1),
            nack_response_delay: Duration::ZERO,
            max_message_size: 16 * 1024,
            fragmentation: Fragmentation::default(),
        }
    }
}

/// How a matched reader is served by a [`ReliableWriter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedReader {
    /// The reader's unicast locators.
    pub unicast: Vec<Locator>,
    /// Its multicast locators, used when it has no unicast one.
    pub multicast: Vec<Locator>,
    /// RELIABLE: the reader gets HEARTBEATs, GAPs and repairs. A
    /// BEST_EFFORT reader is only sent each change once.
    pub reliable: bool,
    /// Whether the reader gets the changes already in the history when it
    /// is matched (TRANSIENT_LOCAL). Otherwise (VOLATILE) it gets only
    /// the changes added after, and is told that the earlier ones are not
    /// relevant to it.
    pub history: bool,
}

#[derive(Debug)]
struct ReaderProxy {
    unicast: Vec<Locator>,
    multicast: Vec<Locator>,
    reliable: bool,
    /// The first sequence number relevant to this reader: those before
    /// were written before it was matched by a VOLATILE writer.
    start: SequenceNumber,
    /// Every sequence number up to this one has been sent (as DATA or GAP).
    highest_sent: SequenceNumber,
    /// Every sequence number up to this one has been acknowledged.
    acked: SequenceNumber,
    /// Sequence numbers the reader asked for again.
    requested: BTreeSet<SequenceNumber>,
    /// Fragments the reader asked for again, by sequence number.
    requested_fragments: BTreeMap<SequenceNumber, BTreeSet<u32>>,
    last_nackfrag: Option<Count>,
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
        self.add_change_at(inline_qos, payload, None)
    }

    /// Like [`Self::add_change`], with a source timestamp that is sent in
    /// an INFO_TS before the DATA.
    pub fn add_change_at(
        &mut self,
        inline_qos: Option<ParameterList>,
        payload: Option<SerializedPayload>,
        timestamp: Option<Time>,
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
                timestamp,
            },
        );
        sn
    }

    /// The sequence number of the last change added (0 if none).
    pub fn last_sn(&self) -> SequenceNumber {
        self.last_sn
    }

    /// The number of changes in the history.
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Removes the oldest change; returns its sequence number.
    pub fn remove_oldest(&mut self) -> Option<SequenceNumber> {
        self.history.pop_first().map(|(sn, _)| sn)
    }

    /// Removes the changes that every matched reader has acknowledged
    /// (or, for a best-effort reader, was sent), which a VOLATILE writer
    /// no longer needs (8.4.2.3.4). Returns how many were removed.
    pub fn remove_acknowledged(&mut self) -> usize {
        let acked = self
            .readers
            .values()
            .map(|r| r.acked)
            .min()
            .unwrap_or(self.last_sn);
        let keep = self.history.split_off(&SequenceNumber(acked.0 + 1));
        let removed = self.history.len();
        self.history = keep;
        removed
    }

    /// Removes a change from the history. Readers that have not got it
    /// yet are sent a GAP instead.
    pub fn remove_change(&mut self, sn: SequenceNumber) -> bool {
        self.history.remove(&sn).is_some()
    }

    /// Whether the change `sn` is in the history.
    pub fn contains(&self, sn: SequenceNumber) -> bool {
        self.history.contains_key(&sn)
    }

    /// The changes in the history, in order.
    pub fn changes(&self) -> impl Iterator<Item = &Change> {
        self.history.values()
    }

    /// Matches a reliable reader (`matched_reader_add`). It is sent the
    /// whole history. Matching a reader again only updates its locators.
    pub fn add_reader(&mut self, guid: Guid, unicast: Vec<Locator>, multicast: Vec<Locator>) {
        self.add_matched_reader(
            guid,
            MatchedReader {
                unicast,
                multicast,
                reliable: true,
                history: true,
            },
        );
    }

    /// Matches a reader (`matched_reader_add`), reliable or best-effort.
    /// Matching a reader again only updates its locators.
    pub fn add_matched_reader(&mut self, guid: Guid, m: MatchedReader) {
        // A VOLATILE writer starts the reader after what is written now
        // (the filter applied when the proxy is made, 8.4.9.2 T1).
        let start = if m.history {
            SequenceNumber(1)
        } else {
            SequenceNumber(self.last_sn.0 + 1)
        };
        let before = SequenceNumber(start.0 - 1);
        let p = self.readers.entry(guid).or_insert(ReaderProxy {
            unicast: Vec::new(),
            multicast: Vec::new(),
            reliable: m.reliable,
            start,
            highest_sent: before,
            acked: before,
            requested: BTreeSet::new(),
            requested_fragments: BTreeMap::new(),
            last_nackfrag: None,
            repair_at: None,
            last_acknack: None,
            heartbeat_due: false,
        });
        p.unicast = m.unicast;
        p.multicast = m.multicast;
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

    /// Whether every matched reader has acknowledged `sn` (a best-effort
    /// reader, once it was sent).
    pub fn is_acked_by_all(&self, sn: SequenceNumber) -> bool {
        self.readers.values().all(|r| r.acked >= sn)
    }

    /// Whether every matched reliable reader has acknowledged every
    /// change written so far (what `wait_for_acknowledgments` waits for).
    pub fn is_all_acked(&self) -> bool {
        self.readers
            .values()
            .all(|r| !r.reliable || r.acked >= self.last_sn)
    }

    /// Handles an ACKNACK from `reader` (8.4.9.2.8). Duplicates (a count
    /// not above the last one) and ACKNACKs from unmatched readers are
    /// ignored.
    pub fn handle_acknack(&mut self, reader: Guid, a: &AckNack, now: Instant) {
        let Some(p) = self.readers.get_mut(&reader).filter(|p| p.reliable) else {
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
        let acked = p.acked;
        p.requested_fragments.retain(|sn, _| *sn > acked);
        if asked && p.repair_at.is_none() {
            p.repair_at = Some(now + self.config.nack_response_delay);
        }
        if !a.final_flag {
            p.heartbeat_due = true;
        }
    }

    /// Handles a NACK_FRAG from `reader` (8.4.14.1): the fragments it
    /// asks for are sent again, like a repair. Duplicates and NACK_FRAGs
    /// from unmatched readers are ignored.
    pub fn handle_nackfrag(&mut self, reader: Guid, n: &NackFrag, now: Instant) {
        let Some(p) = self.readers.get_mut(&reader).filter(|p| p.reliable) else {
            return;
        };
        if p.last_nackfrag.is_some_and(|c| n.count.0 <= c.0) {
            return;
        }
        p.last_nackfrag = Some(n.count);
        let sn = n.writer_sn;
        if sn <= p.acked || sn > self.last_sn {
            return;
        }
        let set = p.requested_fragments.entry(sn).or_default();
        set.extend(n.fragment_number_state.iter().map(|f| f.0));
        if set.is_empty() {
            p.requested_fragments.remove(&sn);
        } else if p.repair_at.is_none() {
            p.repair_at = Some(now + self.config.nack_response_delay);
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
        let frag = self.config.fragmentation;
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
            let p = self.readers.get_mut(&g).expect("listed above");
            let dest = destination(&p.unicast, &p.multicast);
            let mut b = Batcher::new(header, g.prefix, self.config.max_message_size);
            if !p.reliable {
                // Best effort: each new change once, nothing else.
                if p.highest_sent < self.last_sn {
                    let from = SequenceNumber(p.highest_sent.0 + 1);
                    for c in self.history.range(from..).map(|(_, c)| c) {
                        b.push_change(g.entity_id, c, &frag, None);
                    }
                    p.highest_sent = self.last_sn;
                    p.acked = self.last_sn;
                }
                b.finish(&dest, &mut out);
                continue;
            }
            let mut sent_something = false;
            // Repairs.
            if p.repair_at.is_some_and(|t| t <= now) {
                p.repair_at = None;
                let mut missing = Vec::new();
                let requested = std::mem::take(&mut p.requested);
                for sn in &requested {
                    match self.history.get(sn).filter(|_| *sn >= p.start) {
                        Some(c) => b.push_change(g.entity_id, c, &frag, None),
                        None => missing.push(*sn),
                    }
                }
                for (sn, fragments) in std::mem::take(&mut p.requested_fragments) {
                    if requested.contains(&sn) {
                        continue;
                    }
                    match self.history.get(&sn).filter(|_| sn >= p.start) {
                        Some(c) => b.push_change(g.entity_id, c, &frag, Some(&fragments)),
                        None => missing.push(sn),
                    }
                }
                missing.sort();
                for s in gaps(g.entity_id, writer_id, &missing) {
                    b.push(s, 32);
                }
                sent_something = true;
            }
            // New changes.
            if p.highest_sent < self.last_sn {
                let mut next = SequenceNumber(p.highest_sent.0 + 1);
                for (sn, c) in self.history.range(next..) {
                    if *sn > next {
                        b.push(
                            gap(g.entity_id, writer_id, next, SequenceNumber(sn.0 - 1)),
                            32,
                        );
                    }
                    b.push_change(g.entity_id, c, &frag, None);
                    next = SequenceNumber(sn.0 + 1);
                }
                if next <= self.last_sn {
                    b.push(gap(g.entity_id, writer_id, next, self.last_sn), 32);
                }
                p.highest_sent = self.last_sn;
                sent_something = true;
            }
            let unacked = p.acked < self.last_sn;
            let want_hb = p.heartbeat_due || sent_something || (periodic && unacked);
            p.heartbeat_due = false;
            if want_hb {
                // A HEARTBEAT after data asks the reader to answer, so it
                // can ask for what it missed (final flag not set). What
                // was written before a VOLATILE match is not available to
                // this reader.
                self.heartbeat_count = self.heartbeat_count.wrapping_add(1);
                let hb = Heartbeat {
                    final_flag: !unacked,
                    liveliness_flag: false,
                    reader_id: g.entity_id,
                    writer_id,
                    first_sn: first_sn.max(p.start),
                    last_sn: self.last_sn,
                    count: Count(self.heartbeat_count),
                    group_info: None,
                };
                b.push(Submessage::Heartbeat(hb), 32);
            }
            b.finish(&dest, &mut out);
        }
        out
    }

    /// A HEARTBEAT with the final and liveliness flags to every matched
    /// reader, reliable or not: what `assert_liveliness` sends for a
    /// MANUAL_BY_TOPIC writer (8.7.2.2.3). It asks for no reply.
    pub fn liveliness_heartbeat(&mut self) -> Vec<Outgoing> {
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
            let p = &self.readers[&g];
            let dest = destination(&p.unicast, &p.multicast);
            let mut b = Batcher::new(header, g.prefix, self.config.max_message_size);
            self.heartbeat_count = self.heartbeat_count.wrapping_add(1);
            b.push(
                Submessage::Heartbeat(Heartbeat {
                    final_flag: true,
                    liveliness_flag: true,
                    reader_id: g.entity_id,
                    writer_id,
                    first_sn: first_sn.max(p.start),
                    last_sn: self.last_sn,
                    count: Count(self.heartbeat_count),
                    group_info: None,
                }),
                32,
            );
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
        if self
            .readers
            .values()
            .any(|p| p.reliable && p.acked < self.last_sn)
        {
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
    /// What is held, for each writer, of changes that come as DATA_FRAG.
    pub reassembly: ReassemblyLimits,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        ReaderConfig {
            heartbeat_response_delay: Duration::ZERO,
            reassembly: ReassemblyLimits::default(),
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
    /// Changes of which some fragments are in.
    fragments: Reassembler,
    /// What HEARTBEAT_FRAGs said is available, by sequence number, not
    /// yet answered.
    heartbeat_frags: BTreeMap<SequenceNumber, FragmentNumber>,
    last_heartbeat_frag: Option<Count>,
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

    /// Forgets the fragments of changes that were delivered or will not
    /// come.
    fn tidy(&mut self) {
        let next = self.next_expected;
        let mut known = Vec::new();
        for sn in self.fragments.incomplete() {
            if sn < next || self.has(sn) {
                known.push(sn);
            }
        }
        for sn in known {
            self.fragments.remove(sn);
        }
        let pending = &self.pending;
        self.heartbeat_frags.retain(|sn, _| {
            *sn >= next
                && !pending.range(..=*sn).any(|(k, e)| match e {
                    Pending::Change(_) => k == sn,
                    Pending::Irrelevant(end) => sn <= end,
                })
        });
    }

    /// Whether some change the writer has is only partly in, so that
    /// fragments are to be asked for.
    fn wants_fragments(&self) -> bool {
        self.fragments
            .incomplete()
            .any(|sn| sn <= self.available_max)
            || !self.heartbeat_frags.is_empty()
    }

    /// Whether `sn` (at or after `next_expected`) is known.
    fn has(&self, sn: SequenceNumber) -> bool {
        self.pending.range(..=sn).any(|(k, e)| match e {
            Pending::Change(_) => *k == sn,
            Pending::Irrelevant(end) => sn <= *end,
        })
    }

    /// Missing sequence numbers, from `next_expected` up to what the
    /// writer has, at most 256 of them. A change of which some fragments
    /// are in is not missing: its other fragments are asked for with
    /// NACK_FRAG (8.4.14.1).
    fn missing(&self) -> Vec<SequenceNumber> {
        let mut out = Vec::new();
        let mut sn = self.next_expected;
        while sn <= self.available_max && sn.0 < self.next_expected.0 + 256 {
            if !self.has(sn) && !self.fragments.contains(sn) {
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
    nackfrag_count: i32,
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
            nackfrag_count: 0,
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
            fragments: Reassembler::new(self.config.reassembly, Keep::Lowest),
            heartbeat_frags: BTreeMap::new(),
            last_heartbeat_frag: None,
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

    /// Whether matched `writer` has sent anything to this reader yet (a
    /// HEARTBEAT, a GAP or a change): then it has matched this reader
    /// too.
    pub fn heard_from(&self, writer: &Guid) -> bool {
        self.writers.get(writer).is_some_and(|p| {
            p.last_heartbeat.is_some()
                || p.next_expected > SequenceNumber(1)
                || !p.pending.is_empty()
        })
    }

    /// Handles a DATA from `writer` (8.4.12.2.8) and returns the changes
    /// that can now be delivered. DATA from unmatched writers and
    /// duplicates are ignored.
    pub fn handle_data(&mut self, writer: Guid, d: &Data, timestamp: Option<Time>) -> Vec<Change> {
        self.handle_change(Change {
            writer,
            sequence_number: d.writer_sn,
            inline_qos: d.inline_qos.clone(),
            payload: d.serialized_payload.clone(),
            timestamp,
        })
    }

    /// Handles a DATA_FRAG from `writer` (8.4.14.1) and returns the
    /// changes that can now be delivered: once all its fragments are in,
    /// the change is handled like a DATA.
    pub fn handle_data_frag(
        &mut self,
        writer: Guid,
        d: &DataFrag,
        timestamp: Option<Time>,
    ) -> Vec<Change> {
        let Some(p) = self.writers.get_mut(&writer) else {
            return Vec::new();
        };
        let sn = d.writer_sn;
        if sn < p.next_expected || p.has(sn) {
            return Vec::new();
        }
        match p.fragments.add(writer, d, timestamp) {
            Some(c) => self.handle_change(c),
            None => Vec::new(),
        }
    }

    fn handle_change(&mut self, c: Change) -> Vec<Change> {
        let mut out = Vec::new();
        let Some(p) = self.writers.get_mut(&c.writer) else {
            return out;
        };
        let sn = c.sequence_number;
        if sn < p.next_expected || p.has(sn) {
            return out;
        }
        p.fragments.remove(sn);
        p.heartbeat_frags.remove(&sn);
        p.pending.insert(sn, Pending::Change(c));
        p.available_max = p.available_max.max(sn);
        p.drain(&mut out);
        p.tidy();
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
        p.tidy();
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
        p.tidy();
        let liveliness_only = h.liveliness_flag && h.final_flag;
        let wanted = !h.final_flag || !p.missing().is_empty() || p.wants_fragments();
        if !liveliness_only && wanted && p.ack_at.is_none() {
            p.ack_at = Some(now + self.config.heartbeat_response_delay);
        }
        out
    }

    /// Handles a HEARTBEAT_FRAG from `writer` (8.3.8.7): the fragments of
    /// a change it says it has, and this reader misses, are asked for
    /// with NACK_FRAG at the next [`Self::poll`]. Duplicates are ignored.
    pub fn handle_heartbeat_frag(&mut self, writer: Guid, h: &HeartbeatFrag, now: Instant) {
        let Some(p) = self.writers.get_mut(&writer) else {
            return;
        };
        if p.last_heartbeat_frag.is_some_and(|c| h.count.0 <= c.0) {
            return;
        }
        p.last_heartbeat_frag = Some(h.count);
        let sn = h.writer_sn;
        if sn < p.next_expected || p.has(sn) {
            return;
        }
        let last = p.heartbeat_frags.entry(sn).or_insert(h.last_fragment_num);
        *last = (*last).max(h.last_fragment_num);
        if p.ack_at.is_none() {
            p.ack_at = Some(now + self.config.heartbeat_response_delay);
        }
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
            // NACK_FRAG for the changes partly in, which the writer has
            // all of (HEARTBEAT) or some of (HEARTBEAT_FRAG).
            let mut nacks: BTreeMap<SequenceNumber, FragmentNumberSet> = BTreeMap::new();
            for sn in p.fragments.incomplete() {
                if sn <= p.available_max {
                    if let Some(set) = p.fragments.missing(sn, None) {
                        nacks.insert(sn, set);
                    }
                }
            }
            for (sn, last) in std::mem::take(&mut p.heartbeat_frags) {
                let set = if p.fragments.contains(sn) {
                    p.fragments.missing(sn, Some(last))
                } else {
                    // Nothing of it is in: ask for all it has, up to 256.
                    let n = last.0.min(256);
                    let mut set = FragmentNumberSet::new(FragmentNumber(1), n)
                        .expect("base 1, at most 256 bits");
                    for f in 1..=n {
                        set.insert(FragmentNumber(f));
                    }
                    Some(set)
                };
                if let Some(set) = set {
                    nacks.entry(sn).or_insert(set);
                }
            }
            for (sn, set) in nacks {
                self.nackfrag_count = self.nackfrag_count.wrapping_add(1);
                b.push(
                    Submessage::NackFrag(NackFrag {
                        reader_id: self.guid.entity_id,
                        writer_id: g.entity_id,
                        writer_sn: sn,
                        fragment_number_state: set,
                        count: Count(self.nackfrag_count),
                    }),
                    0,
                );
            }
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
                    Submessage::DataFrag(d) => {
                        if !drop.contains(&i) {
                            got.extend(r.handle_data_frag(w, &d, None));
                        }
                        i += 1;
                    }
                    Submessage::Gap(g) => got.extend(r.handle_gap(w, &g)),
                    Submessage::Heartbeat(h) => got.extend(r.handle_heartbeat(w, &h, now)),
                    Submessage::HeartbeatFrag(h) => r.handle_heartbeat_frag(w, &h, now),
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
                match s {
                    Submessage::AckNack(a) => {
                        w.handle_acknack(Guid::new(m.header.guid_prefix, a.reader_id), &a, now)
                    }
                    Submessage::NackFrag(n) => {
                        w.handle_nackfrag(Guid::new(m.header.guid_prefix, n.reader_id), &n, now)
                    }
                    _ => {}
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

    #[test]
    fn best_effort_reader_gets_each_change_once() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, WriterConfig::default());
        let m = MatchedReader {
            unicast: loc(2),
            multicast: vec![],
            reliable: false,
            history: false,
        };
        w.add_matched_reader(rguid(2), m);
        w.add_change(None, payload(1));
        w.add_change(None, payload(2));
        w.remove_change(SequenceNumber(1));
        let s = subs(&w.poll(t));
        // DATA only: no GAP for 1 and no HEARTBEAT.
        assert_eq!(s.len(), 1);
        assert!(matches!(&s[0], Submessage::Data(d) if d.writer_sn == SequenceNumber(2)));
        assert!(w.is_all_acked());
        assert!(w.is_acked_by_all(SequenceNumber(2)));
        assert!(w.poll(t + Duration::from_secs(5)).is_empty());
        assert_eq!(w.next_deadline(), None);
        // ACKNACKs from it are ignored.
        let set = SequenceNumberSet::new(SequenceNumber(1), 1).unwrap();
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
        assert!(w.poll(t).is_empty());
        assert_eq!(w.remove_acknowledged(), 1);
        assert!(w.is_empty());
    }

    #[test]
    fn volatile_match_starts_after_the_history() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, WriterConfig::default());
        w.add_reader(rguid(2), loc(2), vec![]);
        for i in 1..=3 {
            w.add_change(None, payload(i));
        }
        w.poll(t);
        // Reader 2 has not acknowledged, so 1..=3 stay in the history.
        assert_eq!(w.remove_acknowledged(), 0);
        let mut r = ReliableReader::new(rguid(3), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        let m = MatchedReader {
            unicast: loc(3),
            multicast: vec![],
            reliable: true,
            history: false,
        };
        w.add_matched_reader(rguid(3), m);
        // Nothing is sent to it for 1..=3, and its early ACKNACK (which
        // acknowledges nothing) does not bring them either.
        acknacks(&mut w, &r.poll(t), t);
        assert!(w.poll(t).iter().all(|o| o.locators != loc(3)));
        // A request for 2 anyway is not answered with the DATA: for this
        // reader, 1..=3 count as acknowledged.
        let mut set = SequenceNumberSet::new(SequenceNumber(1), 4).unwrap();
        set.insert(SequenceNumber(2));
        w.handle_acknack(
            rguid(3),
            &AckNack {
                final_flag: true,
                reader_id: rguid(3).entity_id,
                writer_id: wguid().entity_id,
                reader_sn_state: set,
                count: Count(100),
            },
            t,
        );
        assert!(
            subs(
                &w.poll(t)
                    .into_iter()
                    .filter(|o| o.locators == loc(3))
                    .collect::<Vec<_>>()
            )
            .iter()
            .all(|s| !matches!(s, Submessage::Data(_)))
        );
        // New changes reach it, with a HEARTBEAT that starts after 3.
        w.add_change(None, payload(4));
        let out: Vec<Outgoing> = w
            .poll(t)
            .into_iter()
            .filter(|o| o.locators == loc(3))
            .collect();
        assert!(subs(&out).iter().any(|s| matches!(s,
            Submessage::Heartbeat(h) if h.first_sn == SequenceNumber(4)
                && h.last_sn == SequenceNumber(4))));
        let got = deliver(&mut r, &out, &[], t);
        let sns: Vec<i64> = got.iter().map(|c| c.sequence_number.0).collect();
        assert_eq!(sns, [4]);
        let Submessage::AckNack(a) = &subs(&r.poll(t))[0] else {
            panic!()
        };
        assert_eq!(a.reader_sn_state.base(), SequenceNumber(5));
        assert!(a.final_flag);
    }

    #[test]
    fn timestamps_go_in_info_ts() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, WriterConfig::default());
        w.add_reader(rguid(2), loc(2), vec![]);
        let ts = Time {
            seconds: 10,
            fraction: 0,
        };
        w.add_change_at(None, payload(1), Some(ts));
        w.add_change_at(None, payload(2), Some(ts));
        w.add_change(None, payload(3));
        let out = w.poll(t);
        let m = Message::decode(&out[0].bytes).unwrap();
        let kinds: Vec<String> = m
            .submessages
            .iter()
            .map(|s| match s {
                Submessage::InfoDestination(_) => "dst".to_owned(),
                Submessage::InfoTimestamp(i) => format!("ts {:?}", i.timestamp.map(|t| t.seconds)),
                Submessage::Data(d) => format!("data {}", d.writer_sn.0),
                Submessage::Heartbeat(_) => "hb".to_owned(),
                _ => "other".to_owned(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "dst",
                "ts Some(10)",
                "data 1",
                "data 2",
                "ts None",
                "data 3",
                "hb"
            ]
        );
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        let got = crate::receiver::receive(&out[0].bytes, rguid(2).prefix)
            .unwrap()
            .1;
        let stamps: Vec<Option<Time>> = got
            .iter()
            .filter(|x| matches!(x.submessage, Submessage::Data(_)))
            .map(|x| x.timestamp)
            .collect();
        assert_eq!(stamps, [Some(ts), Some(ts), None]);
    }

    fn big(n: u8, size: usize) -> Option<SerializedPayload> {
        let mut bytes = vec![0, 1, 0, 0];
        bytes.extend((0..size).map(|i| (i as u8).wrapping_mul(n)));
        Some(SerializedPayload {
            kind: PayloadKind::Data,
            bytes,
        })
    }

    fn frag_config() -> WriterConfig {
        WriterConfig {
            max_message_size: 1200,
            fragmentation: Fragmentation {
                max_datagram_size: 1500,
                fragment_size: 500,
            },
            ..WriterConfig::default()
        }
    }

    #[test]
    fn large_changes_are_fragmented_and_repaired() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, frag_config());
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        w.add_reader(rguid(2), loc(2), vec![]);
        // 1: small, as DATA. 2 and 3: 4004 octets, 9 fragments of 500 in
        // DATA_FRAGs of 2 fragments (each message at most 1200 octets).
        w.add_change(None, payload(1));
        w.add_change(None, big(2, 4000));
        w.add_change(None, big(3, 4000));
        let out = w.poll(t);
        assert!(out.iter().all(|o| o.bytes.len() <= 1200));
        let s = subs(&out);
        let frags: Vec<(i64, u32, u16)> = s
            .iter()
            .filter_map(|s| match s {
                Submessage::DataFrag(d) => Some((
                    d.writer_sn.0,
                    d.fragment_starting_num.0,
                    d.fragments_in_submessage,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            frags[..5],
            [(2, 1, 2), (2, 3, 2), (2, 5, 2), (2, 7, 2), (2, 9, 1)]
        );
        assert_eq!(frags.len(), 10);

        // Lost: the DATA_FRAG with fragments 3-4 of change 2 (index 2
        // counting the DATA), and the first one of change 3 (index 6).
        let got = deliver(&mut r, &out, &[2, 6], t);
        assert_eq!(got.len(), 1);
        let acks = r.poll(t);
        let s = subs(&acks);
        // ACKNACK: 2 and 3 are not missing whole, so the ACKNACK only
        // acknowledges 1; a NACK_FRAG for each.
        let Submessage::AckNack(a) = &s[0] else {
            panic!("{s:?}")
        };
        assert_eq!(a.reader_sn_state.base(), SequenceNumber(2));
        assert_eq!(a.reader_sn_state.iter().count(), 0);
        let nacks: Vec<(i64, Vec<u32>)> = s[1..]
            .iter()
            .map(|s| match s {
                Submessage::NackFrag(n) => (
                    n.writer_sn.0,
                    n.fragment_number_state.iter().map(|f| f.0).collect(),
                ),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(nacks, [(2, vec![3, 4]), (3, vec![1, 2])]);
        acknacks(&mut w, &acks, t);

        // Only the missing fragments come again.
        let out = w.poll(t);
        let s = subs(&out);
        let resent: Vec<(i64, u32, u16)> = s
            .iter()
            .filter_map(|s| match s {
                Submessage::DataFrag(d) => Some((
                    d.writer_sn.0,
                    d.fragment_starting_num.0,
                    d.fragments_in_submessage,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(resent, [(2, 3, 2), (3, 1, 2)]);
        let got = deliver(&mut r, &out, &[], t);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].payload, big(2, 4000));
        assert_eq!(got[1].payload, big(3, 4000));
        acknacks(&mut w, &r.poll(t), t);
        assert!(w.is_all_acked());
    }

    #[test]
    fn a_change_lost_whole_is_nacked_whole() {
        let t = Instant::now();
        let mut w = ReliableWriter::new(wguid(), VendorId::UNKNOWN, frag_config());
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        w.add_reader(rguid(2), loc(2), vec![]);
        w.add_change(None, big(1, 2000));
        let out = w.poll(t);
        // All 3 DATA_FRAGs are lost.
        assert!(deliver(&mut r, &out, &[0, 1, 2], t).is_empty());
        let acks = r.poll(t);
        let s = subs(&acks);
        assert_eq!(s.len(), 1);
        let Submessage::AckNack(a) = &s[0] else {
            panic!()
        };
        assert_eq!(
            a.reader_sn_state.iter().collect::<Vec<_>>(),
            [SequenceNumber(1)]
        );
        acknacks(&mut w, &acks, t);
        let got = deliver(&mut r, &w.poll(t), &[], t);
        assert_eq!(got[0].payload, big(1, 2000));
    }

    #[test]
    fn heartbeat_frag_asks_for_what_the_writer_has() {
        let t = Instant::now();
        let mut r = ReliableReader::new(rguid(2), VendorId::UNKNOWN, ReaderConfig::default());
        r.add_writer(wguid(), loc(1), vec![], t);
        r.poll(t);
        let hb = |count, last| HeartbeatFrag {
            reader_id: rguid(2).entity_id,
            writer_id: wguid().entity_id,
            writer_sn: SequenceNumber(1),
            last_fragment_num: FragmentNumber(last),
            count: Count(count),
        };
        r.handle_heartbeat_frag(wguid(), &hb(1, 3), t);
        // A duplicate is ignored.
        r.handle_heartbeat_frag(wguid(), &hb(1, 9), t);
        let s = subs(&r.poll(t));
        let Submessage::NackFrag(n) = &s[1] else {
            panic!("{s:?}")
        };
        assert_eq!(
            n.fragment_number_state
                .iter()
                .map(|f| f.0)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(r.next_deadline(), None);
    }
}
