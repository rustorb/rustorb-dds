//! `DomainParticipant` and the thread that runs discovery and receives
//! data for it.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustorb_rtps::receiver::receive;
use rustorb_rtps::sedp::{EndpointData, Sedp, SedpConfig, SedpEvent, endpoints_match};
use rustorb_rtps::spdp::{ParticipantData, ParticipantEvent, Spdp, SpdpConfig};
use rustorb_rtps::udp::{Channel, Datagram, Incoming, UdpConfig, UdpTransport};
use rustorb_rtps::{
    EntityId, Guid, GuidPrefix, Locator, SequenceNumber, Submessage, Time, VendorId,
};
use rustorb_typesupport::TopicType;

use crate::error::{Error, Result};
use crate::publisher::Publisher;
use crate::subscriber::{History, Subscriber};
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
}

impl ParticipantConfig {
    /// The defaults for a domain: the system's interfaces, the default
    /// ports, an announcement every 3 s and a lease of 20 s.
    ///
    /// The announcement period is shorter than SPDP's suggested 30 s so
    /// that participants find each other quickly in a small domain.
    pub fn new(domain_id: u32) -> Self {
        ParticipantConfig {
            udp: UdpConfig::new(domain_id),
            announce_period: Duration::from_secs(3),
            lease_duration: Duration::from_secs(20),
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
    pub(crate) last_sn: SequenceNumber,
    /// Matched readers, local and remote.
    pub(crate) matched: BTreeSet<Guid>,
}

pub(crate) struct Inner {
    spdp: Spdp,
    pub(crate) sedp: Sedp,
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
    stop: AtomicBool,
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
    pub(crate) fn add_writer(&self, data: EndpointData) -> Result<()> {
        {
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
            inner.writers.insert(
                guid,
                WriterState {
                    data: data.clone(),
                    last_sn: SequenceNumber(0),
                    matched,
                },
            );
            let events = inner.sedp.add_writer(data);
            apply_sedp_events(inner, events);
        }
        Ok(())
    }

    /// Adds a local reader: announces it, and matches it with the local
    /// writers.
    pub(crate) fn add_reader(&self, data: EndpointData, history: History) -> Result<()> {
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
        inner.readers.insert(
            guid,
            ReaderState {
                data: data.clone(),
                history,
                matched,
                last_sn: HashMap::new(),
                queue: VecDeque::new(),
                listener: None,
            },
        );
        let events = inner.sedp.add_reader(data);
        apply_sedp_events(inner, events);
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
    }

    /// Sends a serialized sample from a local writer to its matched
    /// readers: local ones directly, remote ones as an RTPS message.
    pub(crate) fn write(
        &self,
        writer: Guid,
        payload: Vec<u8>,
        key_hash: Option<[u8; 16]>,
    ) -> Result<()> {
        let timestamp = Time::now();
        let mut locators: Vec<Locator> = Vec::new();
        let mut listeners = Vec::new();
        let sn;
        {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let Some(w) = inner.writers.get_mut(&writer) else {
                return Ok(());
            };
            w.last_sn.0 += 1;
            sn = w.last_sn;
            let matched: Vec<Guid> = w.matched.iter().copied().collect();
            let mut delivered = false;
            for r in matched {
                if r.prefix == self.prefix {
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
                } else if let Some(d) = inner.sedp.remote_reader(&r) {
                    let to = if d.unicast_locators.is_empty() {
                        &d.multicast_locators
                    } else {
                        &d.unicast_locators
                    };
                    for l in to {
                        if !locators.contains(l) {
                            locators.push(*l);
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
        if locators.is_empty() {
            return Ok(());
        }
        let bytes = crate::wire::data_message(
            self.prefix,
            writer.entity_id,
            sn,
            timestamp,
            payload,
            key_hash,
        );
        // Best effort: the write fails only when no reader could be sent
        // to.
        let mut last_err = None;
        let mut sent = false;
        for l in &locators {
            match self.transport.send(&bytes, l) {
                Ok(()) => sent = true,
                Err(e) => last_err = Some(e),
            }
        }
        match last_err {
            Some(e) if !sent => Err(e.into()),
            _ => Ok(()),
        }
    }
}

/// Applies SEDP match events to the local endpoints.
fn apply_sedp_events(inner: &mut Inner, events: Vec<SedpEvent>) {
    for e in events {
        match e {
            SedpEvent::Matched { local, remote } => {
                if let Some(w) = inner.writers.get_mut(&local) {
                    w.matched.insert(remote);
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.matched.insert(remote);
                }
            }
            SedpEvent::Unmatched { local, remote } => {
                if let Some(w) = inner.writers.get_mut(&local) {
                    w.matched.remove(&remote);
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.matched.remove(&remote);
                    r.last_sn.remove(&remote);
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
            stop: AtomicBool::new(false),
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
        for d in datagrams {
            match d.channel {
                Channel::DiscoveryMulticast | Channel::DiscoveryUnicast => {
                    for e in inner.spdp.handle_message(&d.data, now) {
                        participant_event(inner, e, now);
                    }
                    let events = inner.sedp.handle_message(&d.data, now);
                    apply_sedp_events(inner, events);
                }
                Channel::UserMulticast | Channel::UserUnicast => {
                    delivered |= user_data(shared.prefix, inner, &d.data, &mut listeners);
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
        if delivered {
            shared.data_arrived.notify_all();
        }
    }
    for (bytes, to) in out {
        for l in &to {
            // Discovery is repeated, so a datagram that cannot be sent
            // now is sent again later.
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
            apply_sedp_events(inner, events);
        }
    }
}

/// Delivers the DATA of a user-traffic message to the local readers.
/// Returns whether any reader got a sample.
fn user_data(
    local: GuidPrefix,
    inner: &mut Inner,
    bytes: &[u8],
    listeners: &mut Vec<Listener>,
) -> bool {
    let Some((_, received)) = receive(bytes, local) else {
        return false;
    };
    let mut delivered = false;
    for r in received {
        let Submessage::Data(d) = r.submessage else {
            continue;
        };
        // Only values: dispose and unregister are not handled yet.
        let Some(p) = d
            .serialized_payload
            .filter(|p| p.kind == rustorb_rtps::PayloadKind::Data)
        else {
            continue;
        };
        let writer = Guid::new(r.source_prefix, d.writer_id);
        for (rg, st) in inner.readers.iter_mut() {
            if d.reader_id != EntityId::UNKNOWN && d.reader_id != rg.entity_id {
                continue;
            }
            let s = RawSample {
                payload: p.bytes.clone(),
                writer,
                sequence_number: d.writer_sn,
                source_timestamp: r.timestamp,
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
    delivered
}
