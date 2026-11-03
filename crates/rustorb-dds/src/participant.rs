//! `DomainParticipant` and the thread that runs discovery and receives
//! data for it.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustorb_rtps::qos::ReliabilityKind;
use rustorb_rtps::receiver::receive;
use rustorb_rtps::reliable::{
    Change, MatchedReader, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use rustorb_rtps::sedp::{EndpointData, Sedp, SedpConfig, SedpEvent, endpoints_match};
use rustorb_rtps::spdp::{ParticipantData, ParticipantEvent, Spdp, SpdpConfig};
use rustorb_rtps::udp::{Channel, Datagram, Incoming, UdpConfig, UdpTransport};
use rustorb_rtps::{
    EntityId, Guid, GuidPrefix, Locator, Parameter, ParameterList, PayloadKind, SequenceNumber,
    SerializedPayload, Submessage, Time, VendorId,
};
use rustorb_typesupport::TopicType;

use crate::error::{Error, Result};
use crate::publisher::Publisher;
use crate::qos::{DataReaderQos, DataWriterQos, History};
use crate::subscriber::Subscriber;
use crate::topic::Topic;

/// The vendor id sent in RTPS headers. rustorb has no vendor id from the
/// OMG yet, so this is `VENDORID_UNKNOWN` (DDSI-RTPS 2.5, 8.3.3.1.3).
pub const VENDOR_ID: VendorId = VendorId::UNKNOWN;

/// Settings of a [`DomainParticipant`].
#[derive(Clone, Debug)]
pub struct ParticipantConfig {
    /// The UDP settings: interfaces, port numbers, participant id.
    pub udp: UdpConfig,
    /// How often the participant announces itself (SPDP).
    pub announce_period: Duration,
    /// How long other participants keep this one without hearing from it
    /// (SPDP lease duration).
    pub lease_duration: Duration,
    /// The timing of the user writers (HEARTBEAT period, NACK response
    /// delay, message size; DDSI-RTPS 2.5, 8.4.7.1).
    pub writer: WriterConfig,
    /// The timing of the reliable user readers (HEARTBEAT response delay,
    /// 8.4.10.1).
    pub reader: ReaderConfig,
    /// For testing: drop user-traffic datagrams received (DATA,
    /// HEARTBEAT, GAP, ACKNACK) at random, one in `n` on average, as if
    /// they were lost on the way. `None`, the default, drops nothing.
    pub drop_one_in: Option<u32>,
}

impl ParticipantConfig {
    /// The defaults for a domain: the system's interfaces, the default
    /// ports, an announcement every 3 s and a lease of 20 s.
    ///
    /// The announcement period is shorter than SPDP's suggested 30 s so
    /// that participants find each other quickly in a small domain.
    /// Writers send a HEARTBEAT every 200 ms while some reliable reader
    /// has not acknowledged everything.
    pub fn new(domain_id: u32) -> Self {
        ParticipantConfig {
            udp: UdpConfig::new(domain_id),
            announce_period: Duration::from_secs(3),
            lease_duration: Duration::from_secs(20),
            writer: WriterConfig {
                heartbeat_period: Duration::from_millis(200),
                ..WriterConfig::default()
            },
            reader: ReaderConfig::default(),
            drop_one_in: None,
        }
    }

    /// Only this host: every socket on the loopback interface.
    pub fn localhost(domain_id: u32) -> Self {
        let mut c = Self::new(domain_id);
        c.udp.interfaces = vec![Ipv4Addr::LOCALHOST];
        c
    }
}

/// A sample as it arrived, before it is deserialized.
#[derive(Debug)]
pub(crate) struct RawSample {
    pub(crate) payload: Vec<u8>,
    pub(crate) writer: Guid,
    pub(crate) sequence_number: SequenceNumber,
    pub(crate) source_timestamp: Option<Time>,
    pub(crate) read: bool,
}

pub(crate) type Listener = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct ReaderState {
    pub(crate) data: EndpointData,
    pub(crate) history: History,
    /// The reliable protocol state, for a RELIABLE reader: it orders the
    /// changes of each remote writer and asks for what is missing.
    pub(crate) rtps: Option<ReliableReader>,
    /// Matched writers, local and remote.
    pub(crate) matched: BTreeSet<Guid>,
    /// The highest sequence number taken from each writer: a best-effort
    /// reader drops older ones (DDSI-RTPS 2.5, 8.4.11).
    pub(crate) last_sn: HashMap<Guid, SequenceNumber>,
    pub(crate) queue: VecDeque<RawSample>,
    pub(crate) listener: Option<Listener>,
}

impl ReaderState {
    /// Takes a sample in; returns whether it was kept.
    fn accept(&mut self, s: RawSample) -> bool {
        if !self.matched.contains(&s.writer) {
            return false;
        }
        let last = self.last_sn.entry(s.writer).or_insert(SequenceNumber(0));
        if s.sequence_number <= *last {
            return false;
        }
        *last = s.sequence_number;
        if let History::KeepLast(depth) = self.history {
            while self.queue.len() >= depth.max(1) {
                self.queue.pop_front();
            }
        }
        self.queue.push_back(s);
        true
    }
}

pub(crate) struct WriterState {
    pub(crate) data: EndpointData,
    pub(crate) qos: DataWriterQos,
    /// The history and the remote readers. Local readers get each sample
    /// directly when it is written.
    pub(crate) rtps: ReliableWriter,
    /// Matched readers, local and remote.
    pub(crate) matched: BTreeSet<Guid>,
}

impl WriterState {
    /// Whether the history has no room for one more change, so that a
    /// write must wait (a reliable KEEP_ALL writer at `max_samples`).
    fn is_full(&self) -> bool {
        self.qos.reliability.kind == ReliabilityKind::Reliable
            && self.qos.history == History::KeepAll
            && self
                .qos
                .resource_limits
                .max_samples
                .is_some_and(|m| self.rtps.len() >= m.max(1))
    }

    /// Drops what the history no longer needs: changes every reader has
    /// (the writer is VOLATILE), and those beyond the KEEP_LAST depth.
    fn trim(&mut self) {
        self.rtps.remove_acknowledged();
        if let History::KeepLast(depth) = self.qos.history {
            while self.rtps.len() > depth.max(1) {
                self.rtps.remove_oldest();
            }
        }
    }
}

pub(crate) struct Inner {
    spdp: Spdp,
    pub(crate) sedp: Sedp,
    writer_config: WriterConfig,
    reader_config: ReaderConfig,
    next_entity: u32,
    pub(crate) writers: HashMap<Guid, WriterState>,
    pub(crate) readers: HashMap<Guid, ReaderState>,
    deleted: bool,
}

/// What the participant handle and its entities share with the thread.
pub(crate) struct Shared {
    pub(crate) transport: UdpTransport,
    pub(crate) prefix: GuidPrefix,
    domain_id: u32,
    inner: Mutex<Inner>,
    /// Signalled when a reader gets data.
    pub(crate) data_arrived: Condvar,
    /// Signalled when a writer's readers acknowledge changes or go away.
    pub(crate) acknowledged: Condvar,
    stop: AtomicBool,
    drop_one_in: Option<u32>,
    /// The state of the generator that picks the datagrams to drop.
    drop_state: AtomicU64,
}

impl Shared {
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.deleted {
            return Err(Error::AlreadyDeleted);
        }
        Ok(g)
    }

    /// Like [`Shared::lock`], for clean-up that runs even after the
    /// participant is deleted.
    pub(crate) fn lock_any(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether to drop a received user datagram, for
    /// [`ParticipantConfig::drop_one_in`]. A fixed pattern would line up
    /// with the peer's (one datagram for new data, one for a repair, say)
    /// and drop the same kind every time, so this draws from xorshift64.
    fn should_drop(&self) -> bool {
        let Some(n) = self.drop_one_in else {
            return false;
        };
        let mut x = self.drop_state.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.drop_state.store(x, Ordering::Relaxed);
        x % u64::from(n) == 0
    }

    pub(crate) fn new_guid(&self, kind: u8) -> Result<Guid> {
        let mut g = self.lock()?;
        let n = g.next_entity;
        if n >= 1 << 24 {
            return Err(Error::OutOfResources);
        }
        g.next_entity += 1;
        let key = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        Ok(Guid::new(self.prefix, EntityId::user(key, kind)))
    }

    /// Adds a local writer: announces it, and matches it with the local
    /// readers.
    pub(crate) fn add_writer(&self, data: EndpointData, qos: DataWriterQos) -> Result<()> {
        let mut g = self.lock()?;
        let inner = &mut *g;
        let guid = data.guid;
        let mut matched = BTreeSet::new();
        for (rg, r) in inner.readers.iter_mut() {
            if endpoints_match(&data, &r.data) {
                matched.insert(*rg);
                r.matched.insert(guid);
            }
        }
        let rtps = ReliableWriter::new(guid, VENDOR_ID, inner.writer_config.clone());
        inner.writers.insert(
            guid,
            WriterState {
                data: data.clone(),
                qos,
                rtps,
                matched,
            },
        );
        let events = inner.sedp.add_writer(data);
        apply_sedp_events(inner, events, Instant::now());
        Ok(())
    }

    /// Adds a local reader: announces it, and matches it with the local
    /// writers.
    pub(crate) fn add_reader(&self, data: EndpointData, qos: DataReaderQos) -> Result<()> {
        let mut g = self.lock()?;
        let inner = &mut *g;
        let guid = data.guid;
        let mut matched = BTreeSet::new();
        for (wg, w) in inner.writers.iter_mut() {
            if endpoints_match(&w.data, &data) {
                matched.insert(*wg);
                w.matched.insert(guid);
            }
        }
        let rtps = (qos.reliability.kind == ReliabilityKind::Reliable)
            .then(|| ReliableReader::new(guid, VENDOR_ID, inner.reader_config.clone()));
        inner.readers.insert(
            guid,
            ReaderState {
                data: data.clone(),
                history: qos.history,
                rtps,
                matched,
                last_sn: HashMap::new(),
                queue: VecDeque::new(),
                listener: None,
            },
        );
        let events = inner.sedp.add_reader(data);
        apply_sedp_events(inner, events, Instant::now());
        Ok(())
    }

    pub(crate) fn remove_writer(&self, guid: &Guid) {
        let mut g = self.lock_any();
        let inner = &mut *g;
        if inner.writers.remove(guid).is_none() {
            return;
        }
        for r in inner.readers.values_mut() {
            r.matched.remove(guid);
            r.last_sn.remove(guid);
        }
        if !inner.deleted {
            inner.sedp.remove_writer(guid);
        }
    }

    pub(crate) fn remove_reader(&self, guid: &Guid) {
        let mut g = self.lock_any();
        let inner = &mut *g;
        if inner.readers.remove(guid).is_none() {
            return;
        }
        for w in inner.writers.values_mut() {
            w.matched.remove(guid);
        }
        if !inner.deleted {
            inner.sedp.remove_reader(guid);
        }
        drop(g);
        self.acknowledged.notify_all();
    }

    /// Writes a serialized sample from a local writer: delivers it to the
    /// matched local readers, adds it to the writer's history and sends
    /// it to the matched remote readers.
    ///
    /// A reliable KEEP_ALL writer whose history is full waits up to
    /// `max_blocking_time` for its readers to acknowledge changes.
    pub(crate) fn write(
        &self,
        writer: Guid,
        payload: Vec<u8>,
        key_hash: Option<[u8; 16]>,
    ) -> Result<()> {
        let timestamp = Time::now();
        let mut listeners = Vec::new();
        let out;
        {
            let mut g = self.lock()?;
            let Some(blocking) = g
                .writers
                .get(&writer)
                .map(|w| w.qos.reliability.max_blocking_time)
            else {
                return Ok(());
            };
            let deadline = Instant::now() + blocking;
            loop {
                let Some(w) = g.writers.get(&writer) else {
                    return Ok(());
                };
                if !w.is_full() {
                    break;
                }
                let now = Instant::now();
                if now >= deadline {
                    return Err(Error::Timeout);
                }
                g = match self.acknowledged.wait_timeout(g, deadline - now) {
                    Ok((g, _)) => g,
                    Err(e) => e.into_inner().0,
                };
                if g.deleted {
                    return Err(Error::AlreadyDeleted);
                }
            }
            let inner = &mut *g;
            let w = inner.writers.get_mut(&writer).expect("checked above");
            let inline_qos = key_hash.map(|h| {
                let mut q = ParameterList::new();
                q.push(Parameter::new(rustorb_rtps::pl::pid::KEY_HASH, h));
                q
            });
            let sn = w.rtps.add_change_at(
                inline_qos,
                Some(SerializedPayload {
                    kind: PayloadKind::Data,
                    bytes: payload.clone(),
                }),
                Some(timestamp),
            );
            let now = Instant::now();
            out = w.rtps.poll(now);
            w.trim();
            let matched: Vec<Guid> = w.matched.iter().copied().collect();
            let mut delivered = false;
            for r in matched {
                if r.prefix != self.prefix {
                    continue;
                }
                if let Some(st) = inner.readers.get_mut(&r) {
                    let s = RawSample {
                        payload: payload.clone(),
                        writer,
                        sequence_number: sn,
                        source_timestamp: Some(timestamp),
                        read: false,
                    };
                    if st.accept(s) {
                        delivered = true;
                        if let Some(l) = &st.listener {
                            listeners.push(l.clone());
                        }
                    }
                }
            }
            if delivered {
                self.data_arrived.notify_all();
            }
        }
        for l in listeners {
            l();
        }
        // A datagram that cannot be sent now is like one lost on the way:
        // a reliable reader gets it repaired, a best-effort one does not.
        // The write fails only when nothing could be sent at all.
        let mut last_err = None;
        let mut sent = false;
        for o in &out {
            for l in &o.locators {
                match self.transport.send(&o.bytes, l) {
                    Ok(()) => sent = true,
                    Err(e) => last_err = Some(e),
                }
            }
        }
        match last_err {
            Some(e) if !sent => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Waits until every matched reliable reader has acknowledged every
    /// sample `writer` wrote, or `timeout` passes.
    pub(crate) fn wait_for_acknowledgments(&self, writer: &Guid, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut g = self.lock()?;
        loop {
            match g.writers.get(writer) {
                None => return Ok(()),
                Some(w) if w.rtps.is_all_acked() => return Ok(()),
                Some(_) => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout);
            }
            g = match self.acknowledged.wait_timeout(g, deadline - now) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
            if g.deleted {
                return Err(Error::AlreadyDeleted);
            }
        }
    }
}

/// Applies SEDP match events to the local endpoints: a local writer
/// starts or stops sending to a remote reader, a local reliable reader
/// starts or stops tracking a remote writer.
fn apply_sedp_events(inner: &mut Inner, events: Vec<SedpEvent>, now: Instant) {
    for e in events {
        match e {
            SedpEvent::Matched { local, remote } => {
                if let Some(w) = inner.writers.get_mut(&local) {
                    w.matched.insert(remote);
                    if let Some(d) = inner.sedp.remote_reader(&remote) {
                        w.rtps.add_matched_reader(
                            remote,
                            MatchedReader {
                                unicast: d.unicast_locators.clone(),
                                multicast: d.multicast_locators.clone(),
                                reliable: d.reliability.kind == ReliabilityKind::Reliable,
                                history: false,
                            },
                        );
                    }
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.matched.insert(remote);
                    if let (Some(rtps), Some(d)) =
                        (r.rtps.as_mut(), inner.sedp.remote_writer(&remote))
                    {
                        rtps.add_writer(
                            remote,
                            d.unicast_locators.clone(),
                            d.multicast_locators.clone(),
                            now,
                        );
                    }
                }
            }
            SedpEvent::Unmatched { local, remote } => {
                if let Some(w) = inner.writers.get_mut(&local) {
                    w.matched.remove(&remote);
                    w.rtps.remove_reader(&remote);
                    w.trim();
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.matched.remove(&remote);
                    r.last_sn.remove(&remote);
                    if let Some(rtps) = r.rtps.as_mut() {
                        rtps.remove_writer(&remote);
                    }
                }
            }
            _ => {}
        }
    }
}

/// A DDS domain participant (DDS 1.4, 2.2.2.2.1): the entry point that
/// creates topics, publishers and subscribers in one domain.
///
/// It runs a thread for discovery (SPDP and SEDP) and for receiving data.
/// Dropping the participant stops the thread; writers and readers made
/// from it stop working then.
pub struct DomainParticipant {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for DomainParticipant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainParticipant")
            .field("domain_id", &self.shared.domain_id)
            .field("guid_prefix", &self.shared.prefix)
            .finish()
    }
}

impl DomainParticipant {
    /// Joins a domain with the default settings.
    pub fn new(domain_id: u32) -> Result<Self> {
        Self::with_config(ParticipantConfig::new(domain_id))
    }

    /// Joins a domain.
    pub fn with_config(config: ParticipantConfig) -> Result<Self> {
        let domain_id = config.udp.domain_id;
        let (transport, incoming) = UdpTransport::open(&config.udp)?;
        let prefix = GuidPrefix::generate(VENDOR_ID);
        let mut data = ParticipantData::new(prefix, VENDOR_ID);
        data.domain_id = Some(domain_id);
        data.available_builtin_endpoints.0 |= Sedp::BUILTIN_ENDPOINTS;
        data.metatraffic_unicast_locators = transport.discovery_unicast_locators();
        data.metatraffic_multicast_locators = transport.discovery_multicast_locators();
        data.default_unicast_locators = transport.user_unicast_locators();
        data.default_multicast_locators = transport.user_multicast_locators();
        data.lease_duration = rustorb_rtps::Duration::from_std(config.lease_duration);
        let mut spdp_config = SpdpConfig::new(domain_id, transport.discovery_multicast_locators());
        spdp_config.announce_period = config.announce_period;
        let inner = Inner {
            spdp: Spdp::new(data, spdp_config),
            sedp: Sedp::new(prefix, VENDOR_ID, SedpConfig::default()),
            writer_config: config.writer,
            reader_config: config.reader,
            next_entity: 1,
            writers: HashMap::new(),
            readers: HashMap::new(),
            deleted: false,
        };
        let shared = Arc::new(Shared {
            transport,
            prefix,
            domain_id,
            inner: Mutex::new(inner),
            data_arrived: Condvar::new(),
            acknowledged: Condvar::new(),
            stop: AtomicBool::new(false),
            drop_one_in: config.drop_one_in.filter(|n| *n > 0),
            drop_state: AtomicU64::new(
                u64::from_le_bytes(prefix.0[4..12].try_into().expect("8 bytes")) | 1,
            ),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name(format!("rustorb-dds-{domain_id}"))
            .spawn(move || run(s, incoming))?;
        Ok(DomainParticipant {
            shared,
            thread: Some(thread),
        })
    }

    /// The domain id.
    pub fn domain_id(&self) -> u32 {
        self.shared.domain_id
    }

    /// The participant's GUID.
    pub fn guid(&self) -> Guid {
        Guid::new(self.shared.prefix, EntityId::PARTICIPANT)
    }

    /// Creates a topic of type `T`, named `name`.
    pub fn create_topic<T: TopicType>(&self, name: &str) -> Result<Topic<T>> {
        Ok(Topic::new(name, self.shared.clone()))
    }

    /// Creates a publisher in the default partition.
    pub fn create_publisher(&self) -> Result<Publisher> {
        Ok(Publisher::new(self.shared.clone()))
    }

    /// Creates a subscriber in the default partition.
    pub fn create_subscriber(&self) -> Result<Subscriber> {
        Ok(Subscriber::new(self.shared.clone()))
    }

    /// The GUID prefixes of the other participants discovered.
    pub fn discovered_participants(&self) -> Vec<GuidPrefix> {
        let g = self.shared.lock_any();
        let mut v: Vec<GuidPrefix> = g.spdp.participants().map(|p| p.guid_prefix).collect();
        v.sort();
        v
    }
}

impl Drop for DomainParticipant {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let mut g = self.shared.lock_any();
        g.deleted = true;
        drop(g);
        self.shared.data_arrived.notify_all();
        self.shared.acknowledged.notify_all();
    }
}

/// The longest the thread sleeps, so that it sees the stop flag.
const MAX_WAIT: Duration = Duration::from_millis(50);

fn run(shared: Arc<Shared>, incoming: Incoming) {
    while !shared.stop.load(Ordering::Relaxed) {
        let wait = {
            let g = shared.lock_any();
            let next = [g.spdp.next_deadline(), g.sedp.next_deadline()]
                .into_iter()
                .chain(g.writers.values().map(|w| w.rtps.next_deadline()))
                .chain(
                    g.readers
                        .values()
                        .map(|r| r.rtps.as_ref().and_then(|r| r.next_deadline())),
                )
                .flatten()
                .min();
            next.map_or(MAX_WAIT, |t| {
                t.saturating_duration_since(Instant::now()).min(MAX_WAIT)
            })
        };
        let mut datagrams = Vec::new();
        if let Some(d) = incoming.recv_timeout(wait) {
            datagrams.push(d);
            while let Some(d) = incoming.try_recv() {
                datagrams.push(d);
            }
        }
        step(&shared, datagrams);
    }
}

/// Handles received datagrams and sends what discovery has due.
fn step(shared: &Shared, datagrams: Vec<Datagram>) {
    let now = Instant::now();
    let mut listeners: Vec<Listener> = Vec::new();
    let mut out: Vec<(Vec<u8>, Vec<Locator>)> = Vec::new();
    {
        let mut g = shared.lock_any();
        let inner = &mut *g;
        let mut delivered = false;
        let mut acked = false;
        for d in datagrams {
            match d.channel {
                Channel::DiscoveryMulticast | Channel::DiscoveryUnicast => {
                    for e in inner.spdp.handle_message(&d.data, now) {
                        participant_event(inner, e, now);
                    }
                    let events = inner.sedp.handle_message(&d.data, now);
                    apply_sedp_events(inner, events, now);
                }
                Channel::UserMulticast | Channel::UserUnicast => {
                    if shared.should_drop() {
                        continue;
                    }
                    let r = user_traffic(shared.prefix, inner, &d.data, now, &mut listeners);
                    delivered |= r.delivered;
                    acked |= r.acked;
                }
            }
        }
        for e in inner.spdp.remove_expired(now) {
            participant_event(inner, e, now);
        }
        if let Some((bytes, to)) = inner.spdp.poll_announce(now) {
            out.push((bytes, to.to_vec()));
        }
        for o in inner.sedp.poll(now) {
            out.push((o.bytes, o.locators));
        }
        for w in inner.writers.values_mut() {
            for o in w.rtps.poll(now) {
                out.push((o.bytes, o.locators));
            }
            w.trim();
        }
        for r in inner.readers.values_mut() {
            if let Some(rtps) = r.rtps.as_mut() {
                for o in rtps.poll(now) {
                    out.push((o.bytes, o.locators));
                }
            }
        }
        if delivered {
            shared.data_arrived.notify_all();
        }
        if acked {
            shared.acknowledged.notify_all();
        }
    }
    for (bytes, to) in out {
        for l in &to {
            // Discovery and reliable traffic are repeated, so a datagram
            // that cannot be sent now is sent again later.
            let _ = shared.transport.send(&bytes, l);
        }
    }
    for l in listeners {
        l();
    }
}

fn participant_event(inner: &mut Inner, e: ParticipantEvent, now: Instant) {
    match e {
        ParticipantEvent::Discovered(p) | ParticipantEvent::Updated(p) => {
            inner.sedp.add_participant(&p, now);
        }
        ParticipantEvent::Removed(prefix, _) => {
            let events = inner.sedp.remove_participant(&prefix);
            apply_sedp_events(inner, events, now);
        }
    }
}

/// What [`user_traffic`] did.
#[derive(Default)]
struct UserTraffic {
    /// Some reader got a sample.
    delivered: bool,
    /// Some writer got an ACKNACK.
    acked: bool,
}

/// Handles a user-traffic message: DATA, GAP and HEARTBEAT for the local
/// readers, ACKNACK for the local writers.
fn user_traffic(
    local: GuidPrefix,
    inner: &mut Inner,
    bytes: &[u8],
    now: Instant,
    listeners: &mut Vec<Listener>,
) -> UserTraffic {
    let mut result = UserTraffic::default();
    let Some((_, received)) = receive(bytes, local) else {
        return result;
    };
    for r in received {
        let src = r.source_prefix;
        match r.submessage {
            Submessage::AckNack(a) => {
                if let Some(w) = inner.writers.get_mut(&Guid::new(local, a.writer_id)) {
                    w.rtps.handle_acknack(Guid::new(src, a.reader_id), &a, now);
                    w.trim();
                    result.acked = true;
                }
            }
            Submessage::Data(d) => {
                let writer = Guid::new(src, d.writer_id);
                for_readers(
                    inner,
                    d.reader_id,
                    |st| match st.rtps.as_mut() {
                        Some(rtps) => rtps.handle_data(writer, &d, r.timestamp),
                        None => vec![Change {
                            writer,
                            sequence_number: d.writer_sn,
                            inline_qos: None,
                            payload: d.serialized_payload.clone(),
                            timestamp: r.timestamp,
                        }],
                    },
                    &mut result,
                    listeners,
                );
            }
            Submessage::Gap(gap) => {
                let writer = Guid::new(src, gap.writer_id);
                for_readers(
                    inner,
                    gap.reader_id,
                    |st| match st.rtps.as_mut() {
                        Some(rtps) => rtps.handle_gap(writer, &gap),
                        None => Vec::new(),
                    },
                    &mut result,
                    listeners,
                );
            }
            Submessage::Heartbeat(h) => {
                let writer = Guid::new(src, h.writer_id);
                for_readers(
                    inner,
                    h.reader_id,
                    |st| match st.rtps.as_mut() {
                        Some(rtps) => rtps.handle_heartbeat(writer, &h, now),
                        None => Vec::new(),
                    },
                    &mut result,
                    listeners,
                );
            }
            _ => {}
        }
    }
    result
}

/// Runs `f` on the local readers a submessage is for (`reader_id`, or
/// every reader when it is `ENTITYID_UNKNOWN`) and keeps the changes it
/// returns.
fn for_readers(
    inner: &mut Inner,
    reader_id: EntityId,
    mut f: impl FnMut(&mut ReaderState) -> Vec<Change>,
    result: &mut UserTraffic,
    listeners: &mut Vec<Listener>,
) {
    for (rg, st) in inner.readers.iter_mut() {
        if reader_id != EntityId::UNKNOWN && reader_id != rg.entity_id {
            continue;
        }
        for c in f(st) {
            // Only values: dispose and unregister come with instances
            // (rustorb-dds#17).
            let Some(p) = c.payload.filter(|p| p.kind == PayloadKind::Data) else {
                continue;
            };
            let s = RawSample {
                payload: p.bytes,
                writer: c.writer,
                sequence_number: c.sequence_number,
                source_timestamp: c.timestamp,
                read: false,
            };
            if st.accept(s) {
                result.delivered = true;
                if let Some(l) = &st.listener {
                    listeners.push(l.clone());
                }
            }
        }
    }
}
