//! `DomainParticipant` and the thread that runs discovery and receives
//! data for it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustorb_rtps::frag::{Keep, Reassembler, ReassemblyLimits};
use rustorb_rtps::pl::pid;
use rustorb_rtps::qos::{DurabilityKind, LivelinessKind, ReliabilityKind};
use rustorb_rtps::receiver::receive;
use rustorb_rtps::reliable::{
    Change, MatchedReader, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use rustorb_rtps::sedp::{EndpointData, Match, Sedp, SedpConfig, SedpEvent, match_endpoints};
use rustorb_rtps::spdp::{ParticipantData, ParticipantEvent, Spdp, SpdpConfig};
use rustorb_rtps::spdp_secure::SpdpSecure;
use rustorb_rtps::typelookup::{TypeLookup, TypeLookupReplied};
use rustorb_rtps::udp::{Channel, Datagram, Incoming as UdpIncoming, UdpConfig, UdpTransport};
use rustorb_rtps::wlp::{self, Wlp};
use rustorb_rtps::{
    Endianness, EntityId, Guid, GuidPrefix, Locator, Parameter, ParameterList, PayloadKind,
    SampleIdentity, SequenceNumber, SerializedPayload, Submessage, Time, VendorId,
};
use rustorb_typesupport::TopicType;
use rustorb_xtypes::dynamic::{ContentFilter, DynamicData, DynamicType, DynamicTypeBuilderFactory};
use rustorb_xtypes::type_object::{TypeIdentifier, TypeInformation};
use rustorb_xtypes::typelookup::{
    GetTypeDependenciesIn, GetTypesIn, TypeLookupCall, TypeLookupReturn,
};
use rustorb_xtypes::types::{DescribeType, TypeObjects};

use crate::condition::Waker;
use crate::error::{Error, Result};
use crate::instance::{self, Incoming, InstanceHandle, ReaderHistory};
use crate::listener::{
    AnyDataReader, AnyDataWriter, DomainParticipantListener, PublisherEntity, PublisherListener,
    SubscriberEntity, SubscriberListener, TopicListener,
};
use crate::publisher::Publisher;
use crate::qos::{DataReaderQos, DataWriterQos, History, PublisherQos, SubscriberQos};
#[cfg(feature = "security")]
use crate::security::{PeerEvent, Security, SecurityConfig};
use crate::status::{
    DeadlineMissedStatus, Deadlines, IncompatibleQosStatus, InconsistentTopicStatus,
    LivelinessChangedStatus, LivelinessLostStatus, MatchedStatus, SampleLostStatus,
    SampleRejectedStatus, StatusMask,
};
use crate::subscriber::Subscriber;
use crate::topic::{ContentFilteredTopic, Topic};
use crate::typed::{Dynamic, KeyOps, Typed};

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
    /// USER_DATA of the participant (DDS 1.4, 2.2.3.1): octets announced
    /// to the other participants with SPDP. Empty by default. ROS 2
    /// announces `enclave=/;` here.
    pub user_data: Vec<u8>,
    /// Whether to exchange messages with the participants of rustorb-dds
    /// on this host through shared memory rather than UDP (feature `shm`;
    /// true by default). Others are reached by UDP either way.
    pub shared_memory: bool,
    /// DDS Security (feature `security`): the participant's identity,
    /// governance and permissions. `None`, the default, joins without
    /// security.
    #[cfg(feature = "security")]
    pub security: Option<SecurityConfig>,
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
            user_data: Vec::new(),
            shared_memory: true,
            #[cfg(feature = "security")]
            security: None,
        }
    }

    /// Only this host: every socket on the loopback interface.
    pub fn localhost(domain_id: u32) -> Self {
        let mut c = Self::new(domain_id);
        c.udp.interfaces = vec![Ipv4Addr::LOCALHOST];
        c
    }

    /// Takes the PROPERTY of a DomainParticipantQos (DDS Security 1.2,
    /// 7.2.5), as the bindings for other languages give it: the
    /// `dds.sec.*` properties turn on DDS Security with the builtin
    /// plugins (`SecurityConfig::from_properties`); the others are
    /// ignored.
    ///
    /// Fails with [`Error::Security`] when some but not all of the
    /// `dds.sec.auth.*` and `dds.sec.access.*` properties are given, and
    /// with [`Error::Unsupported`] when there are `dds.sec.*` properties
    /// and the feature `security` is off.
    #[cfg_attr(not(feature = "security"), allow(unused_mut))]
    pub fn set_properties<'a>(
        &mut self,
        properties: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<()> {
        let sec: Vec<(&str, &str)> = properties
            .into_iter()
            .filter(|(n, _)| n.starts_with("dds.sec."))
            .collect();
        if sec.is_empty() {
            return Ok(());
        }
        #[cfg(feature = "security")]
        {
            let missing: Vec<&str> = SecurityConfig::PROPERTIES
                .iter()
                .copied()
                .filter(|n| !sec.iter().any(|(k, _)| k == n))
                .collect();
            if !missing.is_empty() {
                return Err(Error::Security(format!(
                    "missing properties: {}",
                    missing.join(", ")
                )));
            }
            self.security = SecurityConfig::from_properties(sec);
            Ok(())
        }
        #[cfg(not(feature = "security"))]
        Err(Error::Unsupported(
            "DDS Security: rustorb-dds was built without the feature `security`",
        ))
    }
}

/// A status change of a reader, with the status, for its listener.
pub(crate) enum ReaderEvent {
    DataAvailable,
    SubscriptionMatched(MatchedStatus),
    LivelinessChanged(LivelinessChangedStatus),
    RequestedDeadlineMissed(DeadlineMissedStatus),
    RequestedIncompatibleQos(IncompatibleQosStatus),
    SampleRejected(SampleRejectedStatus),
    SampleLost(SampleLostStatus),
}

/// A status change of a writer, with the status, for its listener.
pub(crate) enum WriterEvent {
    PublicationMatched(MatchedStatus),
    LivelinessLost(LivelinessLostStatus),
    OfferedDeadlineMissed(DeadlineMissedStatus),
    OfferedIncompatibleQos(IncompatibleQosStatus),
}

/// A reader's listener, with its type erased: it is given the reader's
/// participant and GUID to make a handle of the right type.
pub(crate) type ReaderListener = Arc<dyn Fn(&Arc<Shared>, Guid, ReaderEvent) + Send + Sync>;
/// A writer's listener, with its type erased.
pub(crate) type WriterListener = Arc<dyn Fn(&Arc<Shared>, Guid, WriterEvent) + Send + Sync>;

/// A listener call that [`Shared::flush`] makes after unlocking.
type ListenerCall = Box<dyn FnOnce(&Arc<Shared>)>;

/// Hands a writer's status change to the listener of its publisher or
/// participant.
fn writer_event<L: PublisherListener + ?Sized>(
    l: &L,
    shared: &Arc<Shared>,
    guid: Guid,
    ev: WriterEvent,
) {
    let w = AnyDataWriter {
        shared: shared.clone(),
        guid,
    };
    match ev {
        WriterEvent::PublicationMatched(s) => l.on_publication_matched(&w, s),
        WriterEvent::LivelinessLost(s) => l.on_liveliness_lost(&w, s),
        WriterEvent::OfferedDeadlineMissed(s) => l.on_offered_deadline_missed(&w, s),
        WriterEvent::OfferedIncompatibleQos(s) => l.on_offered_incompatible_qos(&w, s),
    }
}

/// Calls `on_data_on_readers` of the listener of a subscriber or
/// participant.
fn data_on_readers<L: SubscriberListener + ?Sized>(
    l: &L,
    shared: &Arc<Shared>,
    sub: Arc<SubscriberEntity>,
) {
    l.on_data_on_readers(&Subscriber::from_entity(shared.clone(), sub));
}

/// Hands a reader's status change to the listener of its subscriber or
/// participant.
fn reader_event<L: SubscriberListener + ?Sized>(
    l: &L,
    shared: &Arc<Shared>,
    guid: Guid,
    ev: ReaderEvent,
) {
    let r = AnyDataReader {
        shared: shared.clone(),
        guid,
    };
    match ev {
        ReaderEvent::DataAvailable => l.on_data_available(&r),
        ReaderEvent::SubscriptionMatched(s) => l.on_subscription_matched(&r, s),
        ReaderEvent::LivelinessChanged(s) => l.on_liveliness_changed(&r, s),
        ReaderEvent::RequestedDeadlineMissed(s) => l.on_requested_deadline_missed(&r, s),
        ReaderEvent::RequestedIncompatibleQos(s) => l.on_requested_incompatible_qos(&r, s),
        ReaderEvent::SampleRejected(s) => l.on_sample_rejected(&r, s),
        ReaderEvent::SampleLost(s) => l.on_sample_lost(&r, s),
    }
}

/// `StatusInfo_t` flags (DDSI-RTPS 2.5, 9.6.4.9).
const STATUS_DISPOSED: u8 = 0x01;
const STATUS_UNREGISTERED: u8 = 0x02;

pub(crate) struct ReaderState {
    pub(crate) data: EndpointData,
    /// The reliable protocol state, for a RELIABLE reader: it orders the
    /// changes of each remote writer and asks for what is missing.
    pub(crate) rtps: Option<ReliableReader>,
    /// Matched writers, local and remote.
    pub(crate) matched: BTreeSet<Guid>,
    /// For a BEST_EFFORT reader, the changes of each remote writer that
    /// came as DATA_FRAG and are not all in yet (a RELIABLE reader keeps
    /// them in `rtps`).
    pub(crate) fragments: HashMap<Guid, Reassembler>,
    pub(crate) reassembly: ReassemblyLimits,
    /// REQUESTED_INCOMPATIBLE_QOS.
    pub(crate) incompatible: IncompatibleQosStatus,
    /// The liveliness of the matched writers.
    pub(crate) liveliness: HashMap<Guid, WriterLiveliness>,
    /// LIVELINESS_CHANGED.
    pub(crate) liveliness_changed: LivelinessChangedStatus,
    /// DEADLINE: when each alive instance must next get a sample, and
    /// REQUESTED_DEADLINE_MISSED.
    pub(crate) deadlines: Deadlines,
    /// The samples and the instances.
    pub(crate) samples: ReaderHistory,
    pub(crate) keys: Arc<dyn KeyOps>,
    /// SUBSCRIPTION_MATCHED.
    pub(crate) matched_status: MatchedStatus,
    /// The statuses changed since they were last read or given to the
    /// listener (the StatusChangedFlags, DDS 1.4, 2.2.4.2).
    pub(crate) changes: StatusMask,
    /// The statuses its StatusCondition is sensitive to.
    pub(crate) enabled: StatusMask,
    /// The listener and the statuses it is called for.
    pub(crate) listener: Option<(ReaderListener, StatusMask)>,
    /// The subscriber it was created by.
    pub(crate) subscriber: Option<Arc<SubscriberEntity>>,
    /// The filter of a reader of a content filtered topic: the samples
    /// that do not pass it are dropped as they come.
    pub(crate) filter: Option<ContentFilter>,
    /// The security of the participant, when the reader's topic has its
    /// payloads protected: to decode them.
    #[cfg(feature = "security")]
    pub(crate) payload_security: Option<Arc<Security>>,
}

/// What a reader knows of the liveliness of a matched writer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WriterLiveliness {
    kind: LivelinessKind,
    /// `None`: it cannot lose its liveliness (an infinite lease, or an
    /// AUTOMATIC writer of the same participant).
    lease: Option<Duration>,
    last: Instant,
    alive: bool,
}

impl ReaderState {
    fn new(
        data: EndpointData,
        qos: &DataReaderQos,
        keys: Arc<dyn KeyOps>,
        config: &ReaderConfig,
    ) -> Self {
        let rtps = (qos.reliability.kind == ReliabilityKind::Reliable)
            .then(|| ReliableReader::new(data.guid, VENDOR_ID, config.clone()));
        ReaderState {
            data,
            rtps,
            matched: BTreeSet::new(),
            fragments: HashMap::new(),
            reassembly: config.reassembly,
            incompatible: IncompatibleQosStatus::default(),
            liveliness: HashMap::new(),
            liveliness_changed: LivelinessChangedStatus::default(),
            deadlines: Deadlines::new(qos.deadline),
            samples: ReaderHistory::with_limits(qos.history, qos.resource_limits),
            keys,
            matched_status: MatchedStatus::default(),
            changes: StatusMask::NONE,
            enabled: StatusMask::ALL,
            listener: None,
            subscriber: None,
            filter: None,
            #[cfg(feature = "security")]
            payload_security: None,
        }
    }

    /// Takes a change in; returns the statuses it changed:
    /// DATA_AVAILABLE when a sample was added, SAMPLE_REJECTED and
    /// SAMPLE_LOST.
    fn accept(&mut self, c: Incoming) -> StatusMask {
        if !self.matched.contains(&c.writer) {
            return StatusMask::NONE;
        }
        if let (Some(f), instance::Change::Write(bytes)) = (&self.filter, &c.change) {
            // A sample that cannot be read cannot pass the filter.
            let passes =
                DynamicData::from_payload(f.get_type(), bytes).is_ok_and(|d| f.evaluate(&d));
            if !passes {
                return StatusMask::NONE;
            }
        }
        let h = c.instance;
        let value = matches!(c.change, instance::Change::Write(_));
        let rejected = self.samples.rejected_count();
        let lost = self.samples.lost.total_count;
        let mut changes = StatusMask::NONE;
        if self.samples.accept(c) {
            changes |= StatusMask::DATA_AVAILABLE;
            if value {
                self.deadlines.renew(h, Instant::now());
            }
        }
        if self.samples.rejected_count() != rejected {
            changes |= StatusMask::SAMPLE_REJECTED;
        }
        if self.samples.lost.total_count != lost {
            changes |= StatusMask::SAMPLE_LOST;
        }
        changes
    }

    /// Matches a writer; returns whether it was not matched before.
    fn add_match(&mut self, writer: Guid) -> bool {
        let new = self.matched.insert(writer);
        if new {
            self.matched_status.matched(writer);
        }
        new
    }

    /// Unmatches a writer; returns whether it was matched.
    fn remove_match(&mut self, writer: &Guid) -> bool {
        let was = self.matched.remove(writer);
        if was {
            self.matched_status.unmatched(*writer);
        }
        was
    }

    /// A matched writer is gone (deleted, or unmatched): it is
    /// unmatched, and its instances lose it. Returns the statuses
    /// changed.
    fn writer_removed(&mut self, writer: &Guid) -> StatusMask {
        let mut k = StatusMask::NONE;
        if self.writer_unmatched(writer) {
            k |= StatusMask::LIVELINESS_CHANGED;
        }
        if self.remove_match(writer) {
            k |= StatusMask::SUBSCRIPTION_MATCHED;
            if self.samples.writer_gone(writer) {
                k |= StatusMask::DATA_AVAILABLE;
            }
        }
        k
    }

    /// The status of kind `k`, for the listener: reading it resets its
    /// changes. `None` for a kind a reader does not have.
    fn take_event(&mut self, k: StatusMask) -> Option<ReaderEvent> {
        Some(match k {
            StatusMask::DATA_AVAILABLE => ReaderEvent::DataAvailable,
            StatusMask::SUBSCRIPTION_MATCHED => {
                ReaderEvent::SubscriptionMatched(self.matched_status.take())
            }
            StatusMask::LIVELINESS_CHANGED => {
                ReaderEvent::LivelinessChanged(self.liveliness_changed.take())
            }
            StatusMask::REQUESTED_DEADLINE_MISSED => {
                ReaderEvent::RequestedDeadlineMissed(self.deadlines.missed.take())
            }
            StatusMask::REQUESTED_INCOMPATIBLE_QOS => {
                ReaderEvent::RequestedIncompatibleQos(self.incompatible.take())
            }
            StatusMask::SAMPLE_REJECTED => {
                ReaderEvent::SampleRejected(self.samples.take_rejected_status())
            }
            StatusMask::SAMPLE_LOST => ReaderEvent::SampleLost(self.samples.lost.take()),
            _ => return None,
        })
    }

    /// Starts tracking the liveliness of a newly matched writer: it is
    /// alive.
    fn writer_matched(
        &mut self,
        writer: Guid,
        kind: LivelinessKind,
        lease: Option<Duration>,
        now: Instant,
    ) -> bool {
        let l = WriterLiveliness {
            kind,
            lease,
            last: now,
            alive: true,
        };
        if self.liveliness.insert(writer, l).is_none() {
            self.liveliness_changed.update(writer, None, Some(true));
            return true;
        }
        false
    }

    /// Stops tracking a writer that is no longer matched; returns whether
    /// LIVELINESS_CHANGED changed.
    fn writer_unmatched(&mut self, writer: &Guid) -> bool {
        match self.liveliness.remove(writer) {
            Some(l) => {
                self.liveliness_changed.update(*writer, Some(l.alive), None);
                true
            }
            None => false,
        }
    }

    /// A matched writer asserted its liveliness; returns whether it was
    /// not alive.
    fn writer_asserted(&mut self, writer: &Guid, now: Instant) -> bool {
        if let Some(l) = self.liveliness.get_mut(writer) {
            l.last = now;
            if !l.alive {
                l.alive = true;
                self.liveliness_changed
                    .update(*writer, Some(false), Some(true));
                return true;
            }
        }
        false
    }

    /// Writers whose lease passed become not alive, and the instances
    /// they alone wrote lose their writers; deadlines missed are counted.
    /// Returns the statuses changed.
    fn check(&mut self, now: Instant) -> StatusMask {
        let mut changes = StatusMask::NONE;
        let lost: Vec<Guid> = self
            .liveliness
            .iter()
            .filter(|(_, l)| l.alive && l.lease.is_some_and(|d| now > l.last + d))
            .map(|(g, _)| *g)
            .collect();
        for g in lost {
            self.liveliness.get_mut(&g).expect("listed above").alive = false;
            self.liveliness_changed.update(g, Some(true), Some(false));
            changes |= StatusMask::LIVELINESS_CHANGED;
            if self.samples.writer_lost(&g) {
                changes |= StatusMask::DATA_AVAILABLE;
            }
        }
        let samples = &self.samples;
        if self.deadlines.check(now, |h| samples.is_alive(h)) {
            changes |= StatusMask::REQUESTED_DEADLINE_MISSED;
        }
        changes
    }

    /// Reads a change received from a remote writer: its instance from
    /// the in-line key hash, or else from the value or the key payload,
    /// and what it does to the instance. `None` when it cannot be placed.
    fn incoming(&self, c: Change) -> Option<Incoming> {
        let mut key_hash = None;
        let mut status = 0u8;
        let mut related_sample_identity = None;
        if let Some(q) = &c.inline_qos {
            for p in &q.parameters {
                if p.id == pid::RELATED_SAMPLE_IDENTITY {
                    related_sample_identity = SampleIdentity::from_bytes(
                        &p.value,
                        q.endianness().unwrap_or(Endianness::NATIVE),
                    )
                    .ok();
                } else if p.id == pid::KEY_HASH && p.value.len() >= 16 {
                    key_hash = Some(InstanceHandle(p.value[..16].try_into().expect("16 octets")));
                } else if p.id == pid::STATUS_INFO && p.value.len() >= 4 {
                    // An octet array: the flags are in the last octet.
                    status = p.value[3];
                }
            }
        }
        let lifecycle = status & (STATUS_DISPOSED | STATUS_UNREGISTERED) != 0;
        #[allow(unused_mut)]
        let mut payload = c.payload;
        #[cfg(feature = "security")]
        if let (Some(sec), Some(p)) = (&self.payload_security, payload.as_mut()) {
            p.bytes = sec.decode_payload(&c.writer, &p.bytes)?;
        }
        if let Some(p) = payload.as_mut() {
            drop_missing_padding(&mut p.bytes);
        }
        let change = if lifecycle {
            instance::Change::Lifecycle {
                disposed: status & STATUS_DISPOSED != 0,
                unregistered: status & STATUS_UNREGISTERED != 0,
            }
        } else {
            match &payload {
                Some(p) if p.kind == PayloadKind::Data => instance::Change::Write(p.bytes.clone()),
                _ => return None,
            }
        };
        let instance = if !self.keys.keyed() {
            InstanceHandle::KEYLESS
        } else if let Some(h) = key_hash {
            h
        } else {
            match (&change, &payload) {
                (instance::Change::Write(v), _) => self.keys.instance_of_value(v)?,
                (_, Some(p)) => self
                    .samples
                    .last_values()
                    .find(|(_, v)| self.keys.key_matches(&p.bytes, v))
                    .map(|(h, _)| h)?,
                _ => return None,
            }
        };
        Some(Incoming {
            writer: c.writer,
            sequence_number: c.sequence_number,
            source_timestamp: c.timestamp,
            related_sample_identity,
            instance,
            change,
        })
    }
}

/// Clears the padding count of an encapsulation (DDS-XTypes 1.3,
/// 7.6.3.1.2: the last two bits of the options) when the payload does not
/// have the octets it counts. OpenDDS 3.31 counts the octets that pad its
/// data to a multiple of 4, but leaves them out of a DATA that ends the
/// message; read as counted, the data would lose its last octets.
fn drop_missing_padding(bytes: &mut [u8]) {
    if bytes.len() > 4 && bytes[3] & 3 != 0 && (bytes.len() - 4) % 4 != 0 {
        bytes[3] &= !3;
    }
}

/// The vendor id that OpenDDS sends.
const OPENDDS: VendorId = VendorId([0x01, 0x03]);

/// An instance a writer has registered.
#[derive(Debug)]
pub(crate) struct WriterInstance {
    /// Its changes still in the history, oldest first.
    changes: VecDeque<SequenceNumber>,
    /// Its key payload (the key holder in XCDR1, `CDR_LE`), from the
    /// first value written, for the readers that need one with a
    /// dispose or an unregister.
    key: Option<Vec<u8>>,
}

/// What a writer does to an instance.
#[derive(Debug)]
pub(crate) enum WriteOp {
    /// `write`: a value payload, and the sample it answers (sent in-line
    /// as `PID_RELATED_SAMPLE_IDENTITY`).
    Value(Vec<u8>, Option<SampleIdentity>),
    /// `dispose` and/or `unregister_instance`.
    Lifecycle { dispose: bool, unregister: bool },
}

pub(crate) struct WriterState {
    pub(crate) data: EndpointData,
    pub(crate) qos: DataWriterQos,
    keyed: bool,
    /// The topic type's key operations.
    keys: Arc<dyn KeyOps>,
    /// The history and the remote readers. Local readers get each sample
    /// directly when it is written.
    pub(crate) rtps: ReliableWriter,
    /// Matched readers, local and remote.
    pub(crate) matched: BTreeSet<Guid>,
    /// The registered instances.
    pub(crate) instances: BTreeMap<InstanceHandle, WriterInstance>,
    /// The instance of each change in the history.
    change_instance: HashMap<SequenceNumber, InstanceHandle>,
    /// OFFERED_INCOMPATIBLE_QOS.
    pub(crate) incompatible: IncompatibleQosStatus,
    /// When it last asserted its liveliness.
    last_asserted: Instant,
    /// Whether its liveliness is not lost.
    alive: bool,
    /// LIVELINESS_LOST.
    pub(crate) liveliness_lost: LivelinessLostStatus,
    /// DEADLINE: when each registered instance must next be written, and
    /// OFFERED_DEADLINE_MISSED.
    pub(crate) deadlines: Deadlines,
    /// PUBLICATION_MATCHED.
    pub(crate) matched_status: MatchedStatus,
    /// The statuses changed since they were last read or given to the
    /// listener.
    pub(crate) changes: StatusMask,
    /// The statuses its StatusCondition is sensitive to.
    pub(crate) enabled: StatusMask,
    /// The listener and the statuses it is called for.
    pub(crate) listener: Option<(WriterListener, StatusMask)>,
    /// The publisher it was created by.
    pub(crate) publisher: Option<Arc<PublisherEntity>>,
    /// Encodes the payloads, when the topic has them protected.
    #[cfg(feature = "security")]
    pub(crate) payload_encoder: Option<Arc<rustorb_dds_security::crypto::Encoder>>,
}

impl WriterState {
    /// Matches a reader; returns whether it was not matched before.
    fn add_match(&mut self, reader: Guid) -> bool {
        let new = self.matched.insert(reader);
        if new {
            self.matched_status.matched(reader);
        }
        new
    }

    /// Unmatches a reader; returns whether it was matched.
    fn remove_match(&mut self, reader: &Guid) -> bool {
        let was = self.matched.remove(reader);
        if was {
            self.matched_status.unmatched(*reader);
        }
        was
    }

    /// The status of kind `k`, for the listener: reading it resets its
    /// changes.
    fn take_event(&mut self, k: StatusMask) -> Option<WriterEvent> {
        Some(match k {
            StatusMask::PUBLICATION_MATCHED => {
                WriterEvent::PublicationMatched(self.matched_status.take())
            }
            StatusMask::LIVELINESS_LOST => WriterEvent::LivelinessLost(self.liveliness_lost.take()),
            StatusMask::OFFERED_DEADLINE_MISSED => {
                WriterEvent::OfferedDeadlineMissed(self.deadlines.missed.take())
            }
            StatusMask::OFFERED_INCOMPATIBLE_QOS => {
                WriterEvent::OfferedIncompatibleQos(self.incompatible.take())
            }
            _ => return None,
        })
    }

    /// The lease within which a reader takes the writer as alive, as a
    /// local reader sees it: an AUTOMATIC writer of the same participant
    /// is always alive.
    fn local_lease(&self) -> Option<Duration> {
        match self.qos.liveliness.kind {
            LivelinessKind::Automatic => None,
            _ => self.qos.liveliness.lease_duration,
        }
    }

    /// The liveliness of the writer is lost when its lease passes without
    /// an assertion; deadlines missed are counted. Returns the statuses
    /// changed.
    fn check(&mut self, now: Instant) -> StatusMask {
        let mut changes = StatusMask::NONE;
        if let Some(lease) = self.local_lease() {
            if self.alive && now > self.last_asserted + lease {
                self.alive = false;
                self.liveliness_lost.add();
                changes |= StatusMask::LIVELINESS_LOST;
            }
        }
        let instances = &self.instances;
        if self.deadlines.check(now, |h| instances.contains_key(h)) {
            changes |= StatusMask::OFFERED_DEADLINE_MISSED;
        }
        changes
    }

    /// The changes of an instance still in the history.
    fn instance_changes(&mut self, h: &InstanceHandle) -> usize {
        let Some(i) = self.instances.get_mut(h) else {
            return 0;
        };
        let rtps = &self.rtps;
        i.changes.retain(|sn| rtps.contains(*sn));
        i.changes.len()
    }

    /// Whether the history has no room for one more change of instance
    /// `h`, so that a write must wait: a reliable KEEP_ALL writer at
    /// `max_samples` or `max_samples_per_instance`.
    fn is_full(&mut self, h: &InstanceHandle) -> bool {
        if self.qos.reliability.kind != ReliabilityKind::Reliable
            || self.qos.history != History::KeepAll
        {
            return false;
        }
        let limits = self.qos.resource_limits;
        limits
            .max_samples
            .is_some_and(|m| self.rtps.len() >= m.max(1))
            || limits
                .max_samples_per_instance
                .is_some_and(|m| self.instance_changes(h) >= m.max(1))
    }

    fn transient_local(&self) -> bool {
        self.qos.durability >= DurabilityKind::TransientLocal
    }

    /// Drops what the history no longer needs: changes every reader has,
    /// unless the writer is TRANSIENT_LOCAL and keeps them for later
    /// readers, and those beyond the KEEP_LAST depth of their instance.
    fn trim(&mut self) {
        if !self.transient_local() {
            self.rtps.remove_acknowledged();
        }
        if let History::KeepLast(depth) = self.qos.history {
            let handles: Vec<InstanceHandle> = self.instances.keys().copied().collect();
            for h in handles {
                while self.instance_changes(&h) > depth.max(1) {
                    let i = self.instances.get_mut(&h).expect("listed above");
                    if let Some(sn) = i.changes.pop_front() {
                        self.rtps.remove_change(sn);
                    }
                }
            }
        }
        let rtps = &self.rtps;
        self.change_instance.retain(|sn, _| rtps.contains(*sn));
    }

    /// Registers an instance, if it is not already.
    fn register(&mut self, h: InstanceHandle) -> Result<()> {
        if self.instances.contains_key(&h) {
            return Ok(());
        }
        if self
            .qos
            .resource_limits
            .max_instances
            .is_some_and(|m| self.instances.len() >= m)
        {
            return Err(Error::OutOfResources);
        }
        self.instances.insert(
            h,
            WriterInstance {
                changes: VecDeque::new(),
                key: None,
            },
        );
        Ok(())
    }

    /// Adds a change for instance `h` to the history, registering the
    /// instance or unregistering it as `op` says. Returns the change for
    /// the local readers.
    fn add(
        &mut self,
        h: InstanceHandle,
        op: WriteOp,
        timestamp: Time,
        key_payload: bool,
    ) -> Result<(SequenceNumber, instance::Change)> {
        let unregister = matches!(
            op,
            WriteOp::Lifecycle {
                unregister: true,
                ..
            }
        );
        if !self.instances.contains_key(&h) {
            if unregister {
                return Err(Error::PreconditionNotMet);
            }
            self.register(h)?;
        }
        let mut q = ParameterList::new();
        if self.keyed {
            q.push(Parameter::new(pid::KEY_HASH, h.0));
        }
        let (payload, change) = match op {
            WriteOp::Value(v, related) => {
                if self.keyed {
                    let inst = self.instances.get_mut(&h).expect("registered above");
                    if inst.key.is_none() {
                        inst.key = self.keys.key_payload(&v);
                    }
                }
                if let Some(id) = related {
                    // In the byte order the message is sent in.
                    q.push(Parameter::new(
                        pid::RELATED_SAMPLE_IDENTITY,
                        id.to_bytes(Endianness::NATIVE),
                    ));
                }
                #[cfg(feature = "security")]
                let bytes = match &self.payload_encoder {
                    Some(e) => e.encode_payload(&v),
                    None => v.clone(),
                };
                #[cfg(not(feature = "security"))]
                let bytes = v.clone();
                (
                    Some(SerializedPayload {
                        kind: PayloadKind::Data,
                        bytes,
                    }),
                    instance::Change::Write(v),
                )
            }
            WriteOp::Lifecycle {
                dispose,
                unregister,
            } => {
                let mut flags = 0;
                if dispose {
                    flags |= STATUS_DISPOSED;
                }
                if unregister {
                    flags |= STATUS_UNREGISTERED;
                }
                q.push(Parameter::new(pid::STATUS_INFO, [0, 0, 0, flags]));
                // The key hash tells the instance. Fast DDS 2.11.2 takes
                // the wrong instance for a DATA with a key payload, even
                // with the key hash inline, while Cyclone DDS 0.10.4
                // accepts either; OpenDDS 3.31 needs the key payload. So
                // it goes only when `key_payload` asks for it.
                let key = if key_payload && self.keyed {
                    self.instances.get(&h).and_then(|i| i.key.clone())
                } else {
                    None
                };
                (
                    key.map(|bytes| SerializedPayload {
                        kind: PayloadKind::Key,
                        bytes,
                    }),
                    instance::Change::Lifecycle {
                        disposed: dispose,
                        unregistered: unregister,
                    },
                )
            }
        };
        // Cyclone DDS 0.10 drops a DATA with in-line QoS but no key hash,
        // even on a keyless topic (whose key hash is zero): a keyless
        // writer sends it whenever it sends in-line QoS.
        if !self.keyed && !q.parameters.is_empty() {
            q.parameters.insert(0, Parameter::new(pid::KEY_HASH, h.0));
        }
        let inline_qos = (!q.parameters.is_empty()).then_some(q);
        if matches!(change, instance::Change::Write(_)) {
            self.deadlines.renew(h, Instant::now());
        } else {
            // Disposed or unregistered: no more updates are due.
            self.deadlines.remove(&h);
        }
        let sn = self
            .rtps
            .add_change_at(inline_qos, payload, Some(timestamp));
        self.change_instance.insert(sn, h);
        if unregister {
            self.instances.remove(&h);
        } else {
            self.instances
                .get_mut(&h)
                .expect("registered above")
                .changes
                .push_back(sn);
        }
        Ok((sn, change))
    }
}

pub(crate) struct Inner {
    spdp: Spdp,
    pub(crate) sedp: Sedp,
    wlp: Wlp,
    /// The secure liveliness endpoints, under DDS Security liveliness
    /// protection.
    wlp_secure: Option<Wlp>,
    /// The DCPSParticipantsSecure endpoints, under DDS Security
    /// discovery protection.
    spdp_secure: Option<SpdpSecure>,
    typelookup: TypeLookup,
    /// The TypeObjects asked for, and the participant asked: each is
    /// asked for once from each participant.
    type_requests: HashSet<(TypeIdentifier, GuidPrefix)>,
    /// The COMPLETE types whose dependencies were asked for, and of whom.
    dependency_requests: HashSet<(TypeIdentifier, GuidPrefix)>,
    /// How many TypeObjects came by TypeLookup.
    learnt_types: usize,
    /// When to write the next automatic liveliness message, while there
    /// are AUTOMATIC writers with a finite lease.
    next_automatic: Option<Instant>,
    /// A MANUAL_BY_PARTICIPANT writer asserted its liveliness since the
    /// last manual liveliness message.
    manual_pending: bool,
    /// The earliest time for the next manual liveliness message.
    next_manual: Instant,
    writer_config: WriterConfig,
    reader_config: ReaderConfig,
    next_entity: u32,
    pub(crate) writers: HashMap<Guid, WriterState>,
    pub(crate) readers: HashMap<Guid, ReaderState>,
    deleted: bool,
    /// Status changes of local writers and readers, for
    /// [`Shared::flush`] to hand to their listeners, or else to mark as
    /// changed for their StatusConditions.
    pending: Vec<(Guid, StatusMask)>,
    /// The TypeObjects of the registered types, by type name.
    types: HashMap<String, TypeObjects>,
    /// The registered types, by type name, for content filters.
    dynamic_types: HashMap<String, DynamicType>,
    /// INCONSISTENT_TOPIC, by topic name.
    pub(crate) inconsistent_topics: HashMap<String, InconsistentTopicStatus>,
    /// The topics whose INCONSISTENT_TOPIC changed, for [`Shared::flush`].
    pending_topics: Vec<String>,
    /// The listeners of the topics, by topic name.
    pub(crate) topic_listeners: HashMap<String, (Arc<dyn TopicListener>, StatusMask)>,
    /// The participant's listener.
    pub(crate) listener: Option<(Arc<dyn DomainParticipantListener>, StatusMask)>,
    /// DDS Security, when it is on.
    #[cfg(feature = "security")]
    pub(crate) security: Option<Arc<Security>>,
}

/// Writes a liveliness message on the plain and the secure endpoints.
fn assert_wlp(wlp: &mut Wlp, secure: &mut Option<Wlp>, kind: [u8; 4]) {
    wlp.assert_liveliness(kind);
    if let Some(w) = secure.as_mut() {
        w.assert_liveliness(kind);
    }
}

/// The shortest interval between two liveliness messages of a kind.
const MIN_LIVELINESS_PERIOD: Duration = Duration::from_millis(10);

impl Inner {
    /// Announces a local writer, on the secure SEDP endpoints if its
    /// topic has discovery protection.
    fn sedp_add_writer(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        #[cfg(feature = "security")]
        if self
            .security
            .as_ref()
            .is_some_and(|s| s.topic_discovery_protected(&data.topic_name))
        {
            return self.sedp.add_writer_secure(data);
        }
        self.sedp.add_writer(data)
    }

    /// Announces a local reader, like [`Self::sedp_add_writer`].
    fn sedp_add_reader(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        #[cfg(feature = "security")]
        if self
            .security
            .as_ref()
            .is_some_and(|s| s.topic_discovery_protected(&data.topic_name))
        {
            return self.sedp.add_reader_secure(data);
        }
        self.sedp.add_reader(data)
    }

    /// Notes that the statuses `k` of the local writer or reader `g`
    /// changed.
    fn changed(&mut self, g: Guid, k: StatusMask) {
        if !k.is_empty() {
            self.pending.push((g, k));
        }
    }

    /// A local writer asserted its liveliness, by writing or with
    /// `assert_liveliness`. For a MANUAL_BY_PARTICIPANT writer, that
    /// asserts every such writer of the participant (DDS 1.4, 2.2.3.11).
    fn local_writer_asserted(&mut self, writer: Guid, now: Instant) {
        let Some(kind) = self.writers.get(&writer).map(|w| w.qos.liveliness.kind) else {
            return;
        };
        if kind == LivelinessKind::ManualByParticipant {
            self.participant_asserted(now);
        } else {
            self.local_asserted(writer, now);
        }
    }

    /// The participant asserted its liveliness: its MANUAL_BY_PARTICIPANT
    /// writers are alive, and the others are told with a manual
    /// liveliness message (DDSI-RTPS 2.5, 8.4.13.5).
    fn participant_asserted(&mut self, now: Instant) {
        let guids: Vec<Guid> = self
            .writers
            .iter()
            .filter(|(_, w)| w.qos.liveliness.kind == LivelinessKind::ManualByParticipant)
            .map(|(g, _)| *g)
            .collect();
        if !guids.is_empty() {
            self.manual_pending = true;
        }
        for g in guids {
            self.local_asserted(g, now);
        }
    }

    fn local_asserted(&mut self, writer: Guid, now: Instant) {
        if let Some(w) = self.writers.get_mut(&writer) {
            w.last_asserted = now;
            w.alive = true;
        }
        self.remote_writer_asserted(&writer, now);
    }

    /// A writer asserted its liveliness, as seen by the local readers.
    fn remote_writer_asserted(&mut self, writer: &Guid, now: Instant) {
        for (g, r) in self.readers.iter_mut() {
            if r.writer_asserted(writer, now) {
                self.pending.push((*g, StatusMask::LIVELINESS_CHANGED));
            }
        }
    }

    /// A liveliness message from another participant: an automatic one
    /// asserts its AUTOMATIC writers, a manual one those and its
    /// MANUAL_BY_PARTICIPANT writers.
    fn remote_participant_asserted(&mut self, prefix: GuidPrefix, kind: [u8; 4], now: Instant) {
        let manual = kind == wlp::kind::MANUAL_LIVELINESS_UPDATE;
        for (rg, r) in self.readers.iter_mut() {
            let guids: Vec<Guid> = r
                .liveliness
                .iter()
                .filter(|(g, l)| {
                    g.prefix == prefix
                        && (l.kind == LivelinessKind::Automatic
                            || (manual && l.kind == LivelinessKind::ManualByParticipant))
                })
                .map(|(g, _)| *g)
                .collect();
            for g in guids {
                if r.writer_asserted(&g, now) {
                    self.pending.push((*rg, StatusMask::LIVELINESS_CHANGED));
                }
            }
        }
    }

    /// Liveliness and deadlines: leases that passed, deadlines missed,
    /// and the liveliness messages due (DDSI-RTPS 2.5, 8.7.2.2.3).
    fn check_timers(&mut self, now: Instant) {
        for (g, w) in self.writers.iter_mut() {
            let k = w.check(now);
            if !k.is_empty() {
                self.pending.push((*g, k));
            }
        }
        for (g, r) in self.readers.iter_mut() {
            let k = r.check(now);
            if !k.is_empty() {
                self.pending.push((*g, k));
            }
        }
        let min_lease = |kind: LivelinessKind| {
            self.writers
                .values()
                .filter(|w| w.qos.liveliness.kind == kind)
                .filter_map(|w| w.qos.liveliness.lease_duration)
                .min()
        };
        // AUTOMATIC: a message at a rate faster than the shortest lease.
        match min_lease(LivelinessKind::Automatic) {
            None => self.next_automatic = None,
            Some(lease) => {
                if self.next_automatic.is_none_or(|t| t <= now) {
                    assert_wlp(
                        &mut self.wlp,
                        &mut self.wlp_secure,
                        wlp::kind::AUTOMATIC_LIVELINESS_UPDATE,
                    );
                    self.next_automatic = Some(now + (lease / 3).max(MIN_LIVELINESS_PERIOD));
                }
            }
        }
        // MANUAL_BY_PARTICIPANT: a message when some writer asserted,
        // at most once a third of the shortest lease.
        if self.manual_pending && now >= self.next_manual {
            self.manual_pending = false;
            assert_wlp(
                &mut self.wlp,
                &mut self.wlp_secure,
                wlp::kind::MANUAL_LIVELINESS_UPDATE,
            );
            let period = min_lease(LivelinessKind::ManualByParticipant)
                .map_or(Duration::from_secs(1), |l| l / 3);
            self.next_manual = now + period.max(MIN_LIVELINESS_PERIOD);
        }
    }
}

/// What the participant handle and its entities share with the thread.
pub(crate) struct Shared {
    /// The `Arc` this is in, to hand to listeners.
    this: Weak<Shared>,
    /// The WaitSets that have a condition of this participant attached,
    /// to wake when a status changes.
    pub(crate) waitsets: Mutex<Vec<Weak<Waker>>>,
    pub(crate) transport: UdpTransport,
    /// The shared memory transport, if it is on.
    #[cfg(feature = "shm")]
    shm: Option<rustorb_rtps::shm::ShmTransport>,
    pub(crate) prefix: GuidPrefix,
    domain_id: u32,
    inner: Mutex<Inner>,
    /// Signalled when a reader gets data.
    pub(crate) data_arrived: Condvar,
    /// Signalled when a writer's readers acknowledge changes or go away.
    pub(crate) acknowledged: Condvar,
    stop: AtomicBool,
    drop_one_in: Option<u32>,
    /// DDS Security, when it is on.
    #[cfg(feature = "security")]
    pub(crate) security: Option<Arc<Security>>,
    /// The state of the generator that picks the datagrams to drop.
    drop_state: AtomicU64,
}

impl Shared {
    /// Sends a message to one destination, given by its locators:
    /// through shared memory if the peer has a locator of it that can be
    /// used, else to its other locators. Whether some send went, and the
    /// last error.
    pub(crate) fn send(&self, bytes: &[u8], locators: &[Locator]) -> (bool, Option<io::Error>) {
        #[cfg(feature = "security")]
        let encoded;
        #[cfg(feature = "security")]
        let bytes: &[u8] = match &self.security {
            Some(sec) => match sec.encode(bytes) {
                Some(b) => {
                    encoded = b;
                    &encoded
                }
                None => return (false, None),
            },
            None => bytes,
        };
        let mut sent = false;
        let mut last_err = None;
        #[cfg(feature = "shm")]
        if let Some(shm) = &self.shm {
            for l in shm.select(locators) {
                let r = if l.kind == Locator::KIND_SHM {
                    shm.send(bytes, l)
                } else {
                    self.transport.send(bytes, l)
                };
                match r {
                    Ok(()) => sent = true,
                    Err(e) => last_err = Some(e),
                }
            }
            return (sent, last_err);
        }
        for l in locators.iter().filter(|l| l.kind != Locator::KIND_SHM) {
            match self.transport.send(bytes, l) {
                Ok(()) => sent = true,
                Err(e) => last_err = Some(e),
            }
        }
        (sent, last_err)
    }

    /// The vendor of a discovered participant.
    pub(crate) fn vendor_of(&self, prefix: &GuidPrefix) -> Option<VendorId> {
        self.lock_any()
            .spdp
            .participant(prefix)
            .map(|p| p.vendor_id)
    }

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Inner>> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.deleted {
            return Err(Error::AlreadyDeleted);
        }
        Ok(g)
    }

    /// Hands the status changes noted since the last call to the
    /// listeners that take them, and marks the others as changed (DDS
    /// 1.4, 2.2.4.2.1); then wakes the threads waiting for data and the
    /// WaitSets. Called without the lock held, after each change.
    ///
    /// A listener runs on the thread that made the change (the
    /// participant's thread, or the one that wrote), without the lock: it
    /// may call any operation, but should be quick.
    pub(crate) fn flush(&self) {
        let mut calls: Vec<ListenerCall> = Vec::new();
        let mut any = false;
        {
            let mut g = self.lock_any();
            let inner = &mut *g;
            if inner.pending.is_empty() && inner.pending_topics.is_empty() {
                return;
            }
            let participant = inner.listener.clone();
            let participant_for = |bit: StatusMask| {
                participant
                    .as_ref()
                    .filter(|(_, m)| m.contains(bit))
                    .map(|(l, _)| l.clone())
            };
            // The subscribers whose on_data_on_readers is called.
            let mut on_readers: Vec<Arc<SubscriberEntity>> = Vec::new();
            for (guid, k) in std::mem::take(&mut inner.pending) {
                any = true;
                for bit in k.bits() {
                    if let Some(w) = inner.writers.get_mut(&guid) {
                        let own = w
                            .listener
                            .as_ref()
                            .filter(|(_, m)| m.contains(bit))
                            .map(|(l, _)| l.clone());
                        if let Some(l) = own {
                            if let Some(ev) = w.take_event(bit) {
                                w.changes = w.changes & !bit;
                                calls.push(Box::new(move |s| l(s, guid, ev)));
                                continue;
                            }
                        }
                        let group = w.publisher.as_ref().and_then(|p| p.listener_for(bit));
                        let call: Option<ListenerCall> = if let Some(l) = group {
                            w.take_event(bit).map(|ev| -> ListenerCall {
                                Box::new(move |s| writer_event(&*l, s, guid, ev))
                            })
                        } else if let Some(l) = participant_for(bit) {
                            w.take_event(bit).map(|ev| -> ListenerCall {
                                Box::new(move |s| writer_event(&*l, s, guid, ev))
                            })
                        } else {
                            None
                        };
                        match call {
                            Some(c) => {
                                w.changes = w.changes & !bit;
                                calls.push(c);
                            }
                            None => w.changes |= bit,
                        }
                    } else if let Some(r) = inner.readers.get_mut(&guid) {
                        if bit == StatusMask::DATA_AVAILABLE {
                            // 2.2.4.3.2: DATA_ON_READERS first, on the
                            // subscriber's listener, else the
                            // participant's. The reader keeps
                            // DATA_AVAILABLE.
                            let on = StatusMask::DATA_ON_READERS;
                            if let Some(sub) = r.subscriber.clone() {
                                let call: Option<ListenerCall> =
                                    match (sub.listener_for(on), participant_for(on)) {
                                        (Some(l), _) => {
                                            let sub = sub.clone();
                                            Some(Box::new(move |s| data_on_readers(&*l, s, sub)))
                                        }
                                        (None, Some(l)) => {
                                            let sub = sub.clone();
                                            Some(Box::new(move |s| data_on_readers(&*l, s, sub)))
                                        }
                                        (None, None) => None,
                                    };
                                if let Some(c) = call {
                                    r.changes |= bit;
                                    if !on_readers.iter().any(|s| Arc::ptr_eq(s, &sub)) {
                                        on_readers.push(sub);
                                        calls.push(c);
                                    }
                                    continue;
                                }
                            }
                        }
                        let own = r
                            .listener
                            .as_ref()
                            .filter(|(_, m)| m.contains(bit))
                            .map(|(l, _)| l.clone());
                        if let Some(l) = own {
                            if let Some(ev) = r.take_event(bit) {
                                r.changes = r.changes & !bit;
                                calls.push(Box::new(move |s| l(s, guid, ev)));
                                continue;
                            }
                        }
                        let group = r.subscriber.as_ref().and_then(|p| p.listener_for(bit));
                        let call: Option<ListenerCall> = if let Some(l) = group {
                            r.take_event(bit).map(|ev| -> ListenerCall {
                                Box::new(move |s| reader_event(&*l, s, guid, ev))
                            })
                        } else if let Some(l) = participant_for(bit) {
                            r.take_event(bit).map(|ev| -> ListenerCall {
                                Box::new(move |s| reader_event(&*l, s, guid, ev))
                            })
                        } else {
                            None
                        };
                        match call {
                            Some(c) => {
                                r.changes = r.changes & !bit;
                                calls.push(c);
                            }
                            None => r.changes |= bit,
                        }
                    }
                }
            }
            let bit = StatusMask::INCONSISTENT_TOPIC;
            for topic in std::mem::take(&mut inner.pending_topics) {
                let own = inner
                    .topic_listeners
                    .get(&topic)
                    .filter(|(_, m)| m.contains(bit))
                    .map(|(l, _)| l.clone());
                let status = |inner: &mut Inner| {
                    inner
                        .inconsistent_topics
                        .get_mut(&topic)
                        .map(|s| s.take())
                        .unwrap_or_default()
                };
                if let Some(l) = own {
                    let status = status(inner);
                    calls.push(Box::new(move |_| l.on_inconsistent_topic(&topic, status)));
                } else if let Some(l) = participant_for(bit) {
                    let status = status(inner);
                    calls.push(Box::new(move |_| l.on_inconsistent_topic(&topic, status)));
                }
            }
        }
        if any {
            self.data_arrived.notify_all();
            self.wake_waitsets();
        }
        if calls.is_empty() {
            return;
        }
        if let Some(this) = self.this.upgrade() {
            for c in calls {
                c(&this);
            }
        }
    }

    /// Calls the listeners of the readers of `subscriber` that have
    /// DATA_AVAILABLE (`notify_datareaders`, DDS 1.4, 2.2.2.5.2.8): their
    /// own listeners, if they take it.
    pub(crate) fn notify_datareaders(&self, subscriber: &Arc<SubscriberEntity>) {
        let bit = StatusMask::DATA_AVAILABLE;
        let mut calls: Vec<ListenerCall> = Vec::new();
        {
            let mut g = self.lock_any();
            for (guid, r) in g.readers.iter_mut() {
                let mine = r
                    .subscriber
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(s, subscriber));
                if !mine || !r.changes.contains(bit) {
                    continue;
                }
                let own = r
                    .listener
                    .as_ref()
                    .filter(|(_, m)| m.contains(bit))
                    .map(|(l, _)| l.clone());
                if let Some(l) = own {
                    r.changes = r.changes & !bit;
                    let guid = *guid;
                    calls.push(Box::new(move |s| l(s, guid, ReaderEvent::DataAvailable)));
                }
            }
        }
        if let Some(this) = self.this.upgrade() {
            for c in calls {
                c(&this);
            }
        }
    }

    /// Wakes the WaitSets attached to some condition of the participant,
    /// to check their conditions again.
    pub(crate) fn wake_waitsets(&self) {
        let mut w = self.waitsets.lock().unwrap_or_else(|e| e.into_inner());
        w.retain(|w| match w.upgrade() {
            Some(w) => {
                w.wake();
                true
            }
            None => false,
        });
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

    /// The TypeInformation of a registered type.
    pub(crate) fn type_information(
        &self,
        type_name: &str,
    ) -> Result<Option<rustorb_xtypes::type_object::TypeInformation>> {
        Ok(self
            .lock()?
            .types
            .get(type_name)
            .map(|o| o.information.clone()))
    }

    /// Adds a local writer: announces it, and matches it with the local
    /// readers.
    pub(crate) fn add_writer(
        &self,
        #[allow(unused_mut)] mut data: EndpointData,
        qos: DataWriterQos,
        keys: Arc<dyn KeyOps>,
        listener: Option<(WriterListener, StatusMask)>,
        publisher: Arc<PublisherEntity>,
    ) -> Result<()> {
        {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let guid = data.guid;
            let now = Instant::now();
            #[cfg(feature = "security")]
            let payload_encoder = match &inner.security {
                Some(sec) => sec.local_endpoint(&mut data, true)?,
                None => None,
            };
            let rtps = ReliableWriter::new(guid, VENDOR_ID, inner.writer_config.clone());
            let liveliness_kind = qos.liveliness.kind;
            let deadline = qos.deadline;
            let mut w = WriterState {
                data: data.clone(),
                qos,
                keyed: keys.keyed(),
                keys,
                rtps,
                matched: BTreeSet::new(),
                instances: BTreeMap::new(),
                change_instance: HashMap::new(),
                incompatible: IncompatibleQosStatus::default(),
                last_asserted: now,
                alive: true,
                liveliness_lost: LivelinessLostStatus::default(),
                deadlines: Deadlines::new(deadline),
                matched_status: MatchedStatus::default(),
                changes: StatusMask::NONE,
                enabled: StatusMask::ALL,
                listener,
                publisher: Some(publisher),
                #[cfg(feature = "security")]
                payload_encoder,
            };
            let lease = w.local_lease();
            let mut changes = StatusMask::NONE;
            for (rg, r) in inner.readers.iter_mut() {
                match match_endpoints(&data, &r.data, inner.sedp.registry()) {
                    Match::Matched => {
                        w.add_match(*rg);
                        changes |= StatusMask::PUBLICATION_MATCHED;
                        let mut k = StatusMask::NONE;
                        if r.add_match(guid) {
                            k |= StatusMask::SUBSCRIPTION_MATCHED;
                        }
                        if r.writer_matched(guid, liveliness_kind, lease, now) {
                            k |= StatusMask::LIVELINESS_CHANGED;
                        }
                        inner.pending.push((*rg, k));
                    }
                    Match::Incompatible(policies) => {
                        w.incompatible.add(&policies);
                        changes |= StatusMask::OFFERED_INCOMPATIBLE_QOS;
                        r.incompatible.add(&policies);
                        inner
                            .pending
                            .push((*rg, StatusMask::REQUESTED_INCOMPATIBLE_QOS));
                    }
                    // Local types are all known, and a local pair counts
                    // only once remote endpoints are found (DDS 1.4,
                    // 2.2.4.1: INCONSISTENT_TOPIC is about other
                    // participants).
                    Match::Inconsistent | Match::Unresolved(_) | Match::Unrelated => {}
                }
            }
            inner.writers.insert(guid, w);
            inner.changed(guid, changes);
            let events = inner.sedp_add_writer(data);
            apply_sedp_events(inner, events, now);
        }
        self.flush();
        Ok(())
    }

    /// Adds a local reader: announces it, and matches it with the local
    /// writers.
    pub(crate) fn add_reader(
        &self,
        #[allow(unused_mut)] mut data: EndpointData,
        qos: DataReaderQos,
        keys: Arc<dyn KeyOps>,
        listener: Option<(ReaderListener, StatusMask)>,
        filter: Option<ContentFilter>,
        subscriber: Arc<SubscriberEntity>,
    ) -> Result<()> {
        {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let guid = data.guid;
            let now = Instant::now();
            #[cfg(feature = "security")]
            if let Some(sec) = &inner.security {
                sec.local_endpoint(&mut data, false)?;
            }
            let mut r = ReaderState::new(data.clone(), &qos, keys, &inner.reader_config);
            #[cfg(feature = "security")]
            {
                r.payload_security = inner
                    .security
                    .clone()
                    .filter(|s| s.payload_protected(&data.topic_name));
            }
            r.listener = listener;
            r.subscriber = Some(subscriber);
            r.filter = filter;
            let mut changes = StatusMask::NONE;
            // What TRANSIENT_LOCAL writers kept, for a TRANSIENT_LOCAL
            // reader.
            let mut kept = Vec::new();
            for (wg, w) in inner.writers.iter_mut() {
                match match_endpoints(&w.data, &data, inner.sedp.registry()) {
                    Match::Matched => {
                        r.add_match(*wg);
                        changes |= StatusMask::SUBSCRIPTION_MATCHED;
                        if r.writer_matched(*wg, w.qos.liveliness.kind, w.local_lease(), now) {
                            changes |= StatusMask::LIVELINESS_CHANGED;
                        }
                        if w.add_match(guid) {
                            inner.pending.push((*wg, StatusMask::PUBLICATION_MATCHED));
                        }
                        if w.transient_local() && qos.durability >= DurabilityKind::TransientLocal {
                            kept.extend(w.rtps.changes().cloned());
                        }
                    }
                    Match::Incompatible(policies) => {
                        r.incompatible.add(&policies);
                        changes |= StatusMask::REQUESTED_INCOMPATIBLE_QOS;
                        w.incompatible.add(&policies);
                        inner
                            .pending
                            .push((*wg, StatusMask::OFFERED_INCOMPATIBLE_QOS));
                    }
                    Match::Inconsistent | Match::Unresolved(_) | Match::Unrelated => {}
                }
            }
            for c in kept {
                if let Some(c) = r.incoming(c) {
                    changes |= r.accept(c);
                }
            }
            inner.readers.insert(guid, r);
            inner.changed(guid, changes);
            let events = inner.sedp_add_reader(data);
            apply_sedp_events(inner, events, now);
        }
        self.flush();
        Ok(())
    }

    /// The registered type named `type_name`.
    pub(crate) fn dynamic_type(&self, type_name: &str) -> Result<Option<DynamicType>> {
        Ok(self.lock()?.dynamic_types.get(type_name).cloned())
    }

    /// Changes the expression parameters of the filters of the readers
    /// `readers`, and announces them (DDSI-RTPS 2.5, 8.7.3.1: as for a
    /// change of QoS). Readers that are gone are skipped.
    pub(crate) fn set_filter_parameters(
        &self,
        readers: &[Guid],
        parameters: &[String],
    ) -> Result<()> {
        {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let now = Instant::now();
            for guid in readers {
                let Some(r) = inner.readers.get_mut(guid) else {
                    continue;
                };
                if let Some(f) = &mut r.filter {
                    f.set_parameters(parameters)?;
                }
                if let Some(cf) = &mut r.data.content_filter {
                    cf.expression_parameters = parameters.to_vec();
                }
                let data = r.data.clone();
                let events = inner.sedp_add_reader(data);
                apply_sedp_events(inner, events, now);
            }
        }
        self.flush();
        Ok(())
    }

    /// Deletes a local writer. Deleting a writer unregisters its
    /// instances (DDS 1.4, 2.2.3.21), disposing them too when
    /// `autodispose_unregistered_instances` is set; then its readers lose
    /// it.
    pub(crate) fn remove_writer(&self, guid: &Guid) {
        let handles: Vec<InstanceHandle> = {
            let g = self.lock_any();
            match g.writers.get(guid) {
                Some(w) if !g.deleted => w.instances.keys().copied().collect(),
                _ => Vec::new(),
            }
        };
        for h in handles {
            let _ = self.write(
                *guid,
                h,
                WriteOp::Lifecycle {
                    dispose: self.autodispose_of(guid),
                    unregister: true,
                },
                Time::now(),
            );
        }
        {
            let mut g = self.lock_any();
            let inner = &mut *g;
            #[cfg(feature = "security")]
            if let Some(sec) = &inner.security {
                sec.remove_local_endpoint(guid);
            }
            if inner.writers.remove(guid).is_none() {
                return;
            }
            for (rg, r) in inner.readers.iter_mut() {
                let k = r.writer_removed(guid);
                if !k.is_empty() {
                    inner.pending.push((*rg, k));
                }
            }
            if !inner.deleted {
                inner.sedp.remove_writer(guid);
            }
        }
        self.flush();
    }

    /// Registers an instance of a local writer, as a write would.
    pub(crate) fn register_instance(&self, writer: &Guid, h: InstanceHandle) -> Result<()> {
        let mut g = self.lock()?;
        let Some(w) = g.writers.get_mut(writer) else {
            return Ok(());
        };
        w.register(h)
    }

    pub(crate) fn autodispose_of(&self, writer: &Guid) -> bool {
        self.lock_any().writers.get(writer).is_some_and(|w| {
            w.qos
                .writer_data_lifecycle
                .autodispose_unregistered_instances
        })
    }

    pub(crate) fn remove_reader(&self, guid: &Guid) {
        {
            let mut g = self.lock_any();
            let inner = &mut *g;
            if inner.readers.remove(guid).is_none() {
                return;
            }
            #[cfg(feature = "security")]
            if let Some(sec) = &inner.security {
                sec.remove_local_endpoint(guid);
            }
            for (wg, w) in inner.writers.iter_mut() {
                if w.remove_match(guid) {
                    inner.pending.push((*wg, StatusMask::PUBLICATION_MATCHED));
                }
            }
            if !inner.deleted {
                inner.sedp.remove_reader(guid);
            }
        }
        self.acknowledged.notify_all();
        self.flush();
    }

    /// Writes a change of instance `h` from a local writer (a value, a
    /// dispose or an unregister): delivers it to the matched local
    /// readers, adds it to the writer's history and sends it to the
    /// matched remote readers.
    ///
    /// A reliable KEEP_ALL writer whose history is full waits up to
    /// `max_blocking_time` for its readers to acknowledge changes.
    pub(crate) fn write(
        &self,
        writer: Guid,
        h: InstanceHandle,
        op: WriteOp,
        timestamp: Time,
    ) -> Result<SequenceNumber> {
        let out;
        let related = match &op {
            WriteOp::Value(_, r) => *r,
            WriteOp::Lifecycle { .. } => None,
        };
        let sn;
        {
            let mut g = self.lock()?;
            let Some(blocking) = g
                .writers
                .get(&writer)
                .map(|w| w.qos.reliability.max_blocking_time)
            else {
                return Err(Error::AlreadyDeleted);
            };
            let deadline = Instant::now() + blocking;
            loop {
                let Some(w) = g.writers.get_mut(&writer) else {
                    return Err(Error::AlreadyDeleted);
                };
                if !w.is_full(&h) {
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
            let change;
            // A dispose or an unregister carries the key payload when an
            // OpenDDS reader is matched (see `WriterState::add`).
            let key_payload = matches!(op, WriteOp::Lifecycle { .. })
                && w.matched.iter().any(|r| {
                    inner
                        .spdp
                        .participant(&r.prefix)
                        .is_some_and(|p| p.vendor_id == OPENDDS)
                });
            (sn, change) = w.add(h, op, timestamp, key_payload)?;
            let now = Instant::now();
            out = w.rtps.poll(now);
            w.trim();
            inner.local_writer_asserted(writer, now);
            let w = inner.writers.get_mut(&writer).expect("checked above");
            let matched: Vec<Guid> = w.matched.iter().copied().collect();
            for r in matched {
                if r.prefix != self.prefix {
                    continue;
                }
                if let Some(st) = inner.readers.get_mut(&r) {
                    let c = Incoming {
                        writer,
                        sequence_number: sn,
                        source_timestamp: Some(timestamp),
                        related_sample_identity: related,
                        instance: h,
                        change: change.clone(),
                    };
                    let k = st.accept(c);
                    inner.changed(r, k);
                }
            }
        }
        self.flush();
        // A datagram that cannot be sent now is like one lost on the way:
        // a reliable reader gets it repaired, a best-effort one does not.
        // The write fails only when nothing could be sent at all.
        let mut last_err = None;
        let mut sent = false;
        for o in &out {
            let (ok, err) = self.send(&o.bytes, &o.locators);
            sent |= ok;
            if err.is_some() {
                last_err = err;
            }
        }
        match last_err {
            Some(e) if !sent => Err(e.into()),
            _ => Ok(sn),
        }
    }

    /// The discovery data of a writer or reader, local or remote.
    pub(crate) fn endpoint_data(&self, guid: &Guid) -> Option<EndpointData> {
        let g = self.lock_any();
        g.writers
            .get(guid)
            .map(|w| &w.data)
            .or_else(|| g.readers.get(guid).map(|r| &r.data))
            .or_else(|| g.sedp.remote_writer(guid))
            .or_else(|| g.sedp.remote_reader(guid))
            .cloned()
    }

    /// Whether participant `prefix` has acknowledged the latest SEDP
    /// announcement of local writer or reader `local`.
    pub(crate) fn is_announced_to(&self, local: &Guid, prefix: &GuidPrefix) -> bool {
        self.lock_any().sedp.is_announced_to(local, prefix)
    }

    /// Changes the discovery data of a local writer or reader with `f`,
    /// and announces it again.
    pub(crate) fn update_endpoint(
        &self,
        guid: &Guid,
        f: impl FnOnce(&mut EndpointData),
    ) -> Result<()> {
        {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let events = if let Some(w) = inner.writers.get_mut(guid) {
                f(&mut w.data);
                let data = w.data.clone();
                inner.sedp_add_writer(data)
            } else if let Some(r) = inner.readers.get_mut(guid) {
                f(&mut r.data);
                let data = r.data.clone();
                inner.sedp_add_reader(data)
            } else {
                return Err(Error::AlreadyDeleted);
            };
            apply_sedp_events(inner, events, Instant::now());
        }
        self.flush();
        Ok(())
    }

    /// `assert_liveliness` of a local writer. A MANUAL_BY_TOPIC writer
    /// sends a HEARTBEAT with the liveliness flag to its readers
    /// (DDSI-RTPS 2.5, 8.7.2.2.3); a MANUAL_BY_PARTICIPANT one asserts
    /// the participant; it does nothing for an AUTOMATIC writer.
    pub(crate) fn assert_writer_liveliness(&self, writer: &Guid) -> Result<()> {
        let out = {
            let mut g = self.lock()?;
            let inner = &mut *g;
            let Some(kind) = inner.writers.get(writer).map(|w| w.qos.liveliness.kind) else {
                return Ok(());
            };
            if kind == LivelinessKind::Automatic {
                return Ok(());
            }
            inner.local_writer_asserted(*writer, Instant::now());
            match inner.writers.get_mut(writer) {
                Some(w) if kind == LivelinessKind::ManualByTopic => w.rtps.liveliness_heartbeat(),
                _ => Vec::new(),
            }
        };
        self.flush();
        // Like a HEARTBEAT that is lost on the way, one that cannot be
        // sent (to a locator kind not supported here, say) is not an
        // error of the call.
        for o in &out {
            let _ = self.send(&o.bytes, &o.locators);
        }
        Ok(())
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
                #[cfg(feature = "security")]
                if let Some(sec) = &inner.security {
                    if remote.prefix != local.prefix {
                        let writer = inner.writers.contains_key(&local);
                        let data = if writer {
                            inner.sedp.remote_reader(&remote)
                        } else {
                            inner.sedp.remote_writer(&remote)
                        };
                        if !data.is_some_and(|d| sec.remote_allowed(d, !writer)) {
                            continue;
                        }
                        sec.matched(local, remote, writer);
                    }
                }
                if let Some(w) = inner.writers.get_mut(&local) {
                    if w.add_match(remote) {
                        inner.pending.push((local, StatusMask::PUBLICATION_MATCHED));
                    }
                    if let Some(d) = inner.sedp.remote_reader(&remote) {
                        w.rtps.add_matched_reader(
                            remote,
                            MatchedReader {
                                unicast: d.unicast_locators.clone(),
                                multicast: d.multicast_locators.clone(),
                                reliable: d.reliability.kind == ReliabilityKind::Reliable,
                                // A TRANSIENT_LOCAL writer sends what it
                                // kept to the readers that ask for it.
                                history: w.transient_local()
                                    && d.durability >= DurabilityKind::TransientLocal,
                            },
                        );
                    }
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    let mut k = StatusMask::NONE;
                    if r.add_match(remote) {
                        k |= StatusMask::SUBSCRIPTION_MATCHED;
                    }
                    if let Some(d) = inner.sedp.remote_writer(&remote) {
                        if r.writer_matched(
                            remote,
                            d.liveliness.kind,
                            d.liveliness.lease_duration.to_std(),
                            now,
                        ) {
                            k |= StatusMask::LIVELINESS_CHANGED;
                        }
                    }
                    if !k.is_empty() {
                        inner.pending.push((local, k));
                    }
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
                #[cfg(feature = "security")]
                if let Some(sec) = &inner.security {
                    sec.unmatched(local, remote);
                }
                if let Some(w) = inner.writers.get_mut(&local) {
                    if w.remove_match(&remote) {
                        inner.pending.push((local, StatusMask::PUBLICATION_MATCHED));
                    }
                    w.rtps.remove_reader(&remote);
                    w.trim();
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.fragments.remove(&remote);
                    let k = r.writer_removed(&remote);
                    if !k.is_empty() {
                        inner.pending.push((local, k));
                    }
                    if let Some(rtps) = r.rtps.as_mut() {
                        rtps.remove_writer(&remote);
                    }
                }
            }
            SedpEvent::IncompatibleQos {
                local, policies, ..
            } => {
                if let Some(w) = inner.writers.get_mut(&local) {
                    w.incompatible.add(&policies);
                    inner
                        .pending
                        .push((local, StatusMask::OFFERED_INCOMPATIBLE_QOS));
                } else if let Some(r) = inner.readers.get_mut(&local) {
                    r.incompatible.add(&policies);
                    inner
                        .pending
                        .push((local, StatusMask::REQUESTED_INCOMPATIBLE_QOS));
                }
            }
            SedpEvent::InconsistentTopic { local, .. } => {
                let topic = if let Some(w) = inner.writers.get(&local) {
                    Some(w.data.topic_name.clone())
                } else {
                    inner.readers.get(&local).map(|r| r.data.topic_name.clone())
                };
                if let Some(topic) = topic {
                    inner
                        .inconsistent_topics
                        .entry(topic.clone())
                        .or_default()
                        .add();
                    inner.pending_topics.push(topic);
                }
            }
            _ => {}
        }
    }
    request_missing_types(inner);
}

/// Asks the participants whose endpoints announced types whose TypeObjects
/// are missing for them (TypeLookup getTypes, DDS-XTypes 1.3, 7.6.3.3.4),
/// with the other missing types their TypeInformation lists. When the
/// TypeInformation does not list all the types the type depends on, asks
/// for those too (getTypeDependencies).
fn request_missing_types(inner: &mut Inner) {
    let mut asks: BTreeMap<GuidPrefix, Vec<TypeIdentifier>> = BTreeMap::new();
    let mut dependencies: BTreeMap<GuidPrefix, Vec<TypeIdentifier>> = BTreeMap::new();
    for (id, remote) in inner.sedp.unresolved_types() {
        let registry = inner.sedp.registry();
        let info = inner
            .sedp
            .remote_writer(&remote)
            .or_else(|| inner.sedp.remote_reader(&remote))
            .and_then(|d| d.type_information.as_ref());
        let mut ids = vec![id];
        if let Some(info) = info {
            ids.extend(registry.missing(info));
            let m = &info.minimal;
            let listed = i32::try_from(m.dependent_typeids.len()).unwrap_or(i32::MAX);
            let root = &m.typeid_with_size.type_id;
            if m.dependent_typeid_count > listed
                && inner.type_requests.insert((root.clone(), remote.prefix))
            {
                dependencies
                    .entry(remote.prefix)
                    .or_default()
                    .push(root.clone());
            }
        }
        for id in ids {
            if !registry.knows(&id) && inner.type_requests.insert((id.clone(), remote.prefix)) {
                asks.entry(remote.prefix).or_default().push(id);
            }
        }
    }
    for (server, type_ids) in asks {
        inner
            .typelookup
            .request(server, TypeLookupCall::GetTypes(GetTypesIn { type_ids }));
    }
    for (server, type_ids) in dependencies {
        inner.typelookup.request(
            server,
            TypeLookupCall::GetTypeDependencies(GetTypeDependenciesIn {
                type_ids,
                continuation_point: None,
            }),
        );
    }
}

/// Asks `server` for the COMPLETE TypeObjects of the type of `info` and
/// of those it depends on that are missing, for
/// [`DomainParticipant::discover_topic_type`]; and for the types it
/// depends on when `info` does not list them all.
fn request_complete_types(inner: &mut Inner, server: GuidPrefix, info: &TypeInformation) {
    let c = &info.complete;
    let root = &c.typeid_with_size.type_id;
    let registry = inner.sedp.registry();
    let requests = &mut inner.type_requests;
    let type_ids: Vec<TypeIdentifier> = std::iter::once(&c.typeid_with_size)
        .chain(&c.dependent_typeids)
        .map(|t| &t.type_id)
        .filter(|id| !registry.knows(id))
        .filter(|id| requests.insert(((*id).clone(), server)))
        .cloned()
        .collect();
    let listed = i32::try_from(c.dependent_typeids.len()).unwrap_or(i32::MAX);
    let dependencies = c.dependent_typeid_count > listed
        && inner.dependency_requests.insert((root.clone(), server));
    if !type_ids.is_empty() {
        inner
            .typelookup
            .request(server, TypeLookupCall::GetTypes(GetTypesIn { type_ids }));
    }
    if dependencies {
        inner.typelookup.request(
            server,
            TypeLookupCall::GetTypeDependencies(GetTypeDependenciesIn {
                type_ids: vec![root.clone()],
                continuation_point: None,
            }),
        );
    }
}

/// Takes in a TypeLookup reply: keeps the TypeObjects it brings and
/// matches again the endpoints that waited for them, or asks for the
/// types it says a type depends on.
fn type_lookup_replied(inner: &mut Inner, r: TypeLookupReplied, now: Instant) {
    match r.reply.data {
        TypeLookupReturn::GetTypes(Ok(out)) => {
            // Each TypeObject is checked against its TypeIdentifier.
            let added = inner.sedp.registry_mut().add_pairs(&out.types).unwrap_or(0);
            inner.learnt_types += added;
            if added > 0 {
                let events = inner.sedp.rematch_unresolved();
                apply_sedp_events(inner, events, now);
            }
        }
        TypeLookupReturn::GetTypeDependencies(Ok(out)) if r.call.is_some() => {
            let registry = inner.sedp.registry();
            let type_ids: Vec<TypeIdentifier> = out
                .dependent_typeids
                .into_iter()
                .map(|t| t.type_id)
                .filter(|id| !registry.knows(id))
                .collect();
            let type_ids: Vec<TypeIdentifier> = type_ids
                .into_iter()
                .filter(|id| inner.type_requests.insert((id.clone(), r.server)))
                .collect();
            if !type_ids.is_empty() {
                inner
                    .typelookup
                    .request(r.server, TypeLookupCall::GetTypes(GetTypesIn { type_ids }));
            }
        }
        _ => {}
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
        #[allow(unused_mut)]
        let mut prefix = GuidPrefix::generate(VENDOR_ID);
        #[cfg(feature = "security")]
        let security = match &config.security {
            Some(c) => {
                let s = Security::new(
                    c,
                    domain_id,
                    prefix,
                    SedpConfig::default().writer,
                    SedpConfig::default().reader,
                )?;
                prefix = s.prefix();
                Some(Arc::new(s))
            }
            None => None,
        };
        let mut data = ParticipantData::new(prefix, VENDOR_ID);
        data.domain_id = Some(domain_id);
        data.available_builtin_endpoints.0 |=
            Sedp::BUILTIN_ENDPOINTS | Wlp::BUILTIN_ENDPOINTS | TypeLookup::BUILTIN_ENDPOINTS;
        data.metatraffic_unicast_locators = transport.discovery_unicast_locators();
        data.metatraffic_multicast_locators = transport.discovery_multicast_locators();
        data.default_unicast_locators = transport.user_unicast_locators();
        data.default_multicast_locators = transport.user_multicast_locators();
        #[cfg(feature = "shm")]
        let shm = if config.shared_memory {
            // Without shared memory (no /dev/shm, say), UDP alone.
            rustorb_rtps::shm::ShmTransport::open(
                &rustorb_rtps::shm::ShmConfig::default(),
                prefix,
                transport.injector(),
            )
            .ok()
        } else {
            None
        };
        #[cfg(feature = "shm")]
        if let Some(shm) = &shm {
            data.metatraffic_unicast_locators
                .push(shm.metatraffic_locator());
            data.default_unicast_locators.push(shm.user_locator());
        }
        data.lease_duration = rustorb_rtps::Duration::from_std(config.lease_duration);
        data.user_data = config.user_data.clone();
        #[cfg(feature = "security")]
        if let Some(s) = &security {
            s.announce(&mut data);
        }
        let mut spdp_config = SpdpConfig::new(domain_id, transport.discovery_multicast_locators());
        spdp_config.announce_period = config.announce_period;
        let inner = Inner {
            spdp: Spdp::new(data, spdp_config),
            sedp: Sedp::new(prefix, VENDOR_ID, SedpConfig::default()),
            wlp: Wlp::new(
                prefix,
                VENDOR_ID,
                SedpConfig::default().writer,
                SedpConfig::default().reader,
            ),
            wlp_secure: None,
            spdp_secure: None,
            typelookup: TypeLookup::new(
                prefix,
                VENDOR_ID,
                SedpConfig::default().writer,
                SedpConfig::default().reader,
            ),
            type_requests: HashSet::new(),
            dependency_requests: HashSet::new(),
            learnt_types: 0,
            next_automatic: None,
            manual_pending: false,
            next_manual: Instant::now(),
            writer_config: config.writer,
            reader_config: config.reader,
            next_entity: 1,
            writers: HashMap::new(),
            readers: HashMap::new(),
            deleted: false,
            pending: Vec::new(),
            types: HashMap::new(),
            dynamic_types: HashMap::new(),
            inconsistent_topics: HashMap::new(),
            pending_topics: Vec::new(),
            topic_listeners: HashMap::new(),
            listener: None,
            #[cfg(feature = "security")]
            security: security.clone(),
        };
        #[cfg(feature = "security")]
        let inner = {
            let mut inner = inner;
            if let Some(s) = &inner.security {
                if s.discovery_protected() {
                    inner.sedp.enable_secure();
                    let mut secure = SpdpSecure::new(
                        prefix,
                        VENDOR_ID,
                        SedpConfig::default().writer,
                        SedpConfig::default().reader,
                    );
                    secure.write(inner.spdp.local());
                    inner.spdp_secure = Some(secure);
                }
                if s.liveliness_protected() {
                    inner.wlp_secure = Some(Wlp::new_secure(
                        prefix,
                        VENDOR_ID,
                        SedpConfig::default().writer,
                        SedpConfig::default().reader,
                    ));
                }
            }
            inner
        };
        let shared = Arc::new_cyclic(|this| Shared {
            this: this.clone(),
            waitsets: Mutex::new(Vec::new()),
            transport,
            #[cfg(feature = "shm")]
            shm,
            prefix,
            domain_id,
            inner: Mutex::new(inner),
            data_arrived: Condvar::new(),
            acknowledged: Condvar::new(),
            stop: AtomicBool::new(false),
            drop_one_in: config.drop_one_in.filter(|n| *n > 0),
            #[cfg(feature = "security")]
            security,
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

    /// Registers the type `T` with its DDS-XTypes description
    /// (`register_type`, DDS-XTypes 1.3, 7.6.3.2): the writers and readers
    /// of `T` created afterwards announce its TypeInformation, and are
    /// matched with remote endpoints by type assignability rather than by
    /// type name (see [`crate::DataReaderQos::type_consistency`]).
    /// rustorb-idl writes [`DescribeType`] with `-Wbxtypes`.
    ///
    /// Registering a type again does nothing. Fails with
    /// [`Error::Unsupported`] for a type that has no TypeObjects (one that
    /// depends on itself).
    pub fn register_type<T: TopicType + DescribeType>(&self) -> Result<()> {
        let objects = TypeObjects::of::<T>()
            .map_err(|_| Error::Unsupported("a type that cannot be described"))?;
        let ty = DynamicType::new(T::describe_type());
        self.add_type(T::TYPE_NAME, objects, ty).map(|_| ())
    }

    /// Registers a type known at run time (`register_type` with a
    /// DynamicType, DDS-XTypes 1.3, 7.6.3.2), under its name: as
    /// [`register_type`](Self::register_type) does for a type the program
    /// was built with. Registering the same type again does nothing; a
    /// different type of a name already registered fails with
    /// [`Error::PreconditionNotMet`].
    pub fn register_dynamic_type(&self, ty: &DynamicType) -> Result<()> {
        let objects = TypeObjects::new(ty.as_type())?;
        if !self.add_type(&ty.get_name(), objects, ty.clone())? {
            return Err(Error::PreconditionNotMet);
        }
        Ok(())
    }

    /// Registers `objects` under `name`. False when another type has that
    /// name.
    fn add_type(&self, name: &str, objects: TypeObjects, ty: DynamicType) -> Result<bool> {
        {
            let mut g = self.shared.lock()?;
            let inner = &mut *g;
            if let Some(known) = inner.types.get(name) {
                return Ok(known.information == objects.information);
            }
            inner.sedp.registry_mut().add_type(&objects);
            inner.types.insert(name.to_owned(), objects);
            inner.dynamic_types.insert(name.to_owned(), ty);
            let events = inner.sedp.rematch_unresolved();
            apply_sedp_events(inner, events, Instant::now());
        }
        self.shared.flush();
        Ok(true)
    }

    /// Creates a topic named `name` of a type known at run time, whose
    /// samples are [`DynamicData`]: its writers and readers are a
    /// `DynamicDataWriter` and a `DynamicDataReader` (DDS-XTypes 1.3,
    /// 7.5.2.12). The type is registered as by
    /// [`register_dynamic_type`](Self::register_dynamic_type), so that
    /// endpoints match by type assignability.
    pub fn create_dynamic_topic(&self, name: &str, ty: &DynamicType) -> Result<Topic<DynamicData>> {
        self.register_dynamic_type(ty)?;
        Ok(Topic::new(
            name,
            self.shared.clone(),
            Arc::new(Dynamic::new(ty)),
        ))
    }

    /// The type of the topic `topic_name` as the other participants know
    /// it, for a program that was not built with it: the type of a remote
    /// writer (or else reader) of the topic that announced its
    /// TypeInformation, made from its COMPLETE TypeObjects, which the
    /// participant asks the remote participant for with the TypeLookup
    /// service (DDS-XTypes 1.3, 7.6.3.3). `None` when no such endpoint
    /// was discovered, or its TypeObjects did not come, within `timeout`.
    ///
    /// Then [`create_dynamic_topic`](Self::create_dynamic_topic) makes a
    /// topic of it, whose readers read the samples of that writer.
    pub fn discover_topic_type(
        &self,
        topic_name: &str,
        timeout: Duration,
    ) -> Result<Option<DynamicType>> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut g = self.shared.lock()?;
                let inner = &mut *g;
                let infos: Vec<(GuidPrefix, TypeInformation)> = inner
                    .sedp
                    .remote_writers()
                    .chain(inner.sedp.remote_readers())
                    .filter(|d| d.topic_name == topic_name)
                    .filter_map(|d| Some((d.guid.prefix, d.type_information.clone()?)))
                    .collect();
                for (_, info) in &infos {
                    let root = &info.complete.typeid_with_size.type_id;
                    if let Ok(ty) = DynamicTypeBuilderFactory::create_type_w_type_identifier(
                        root,
                        inner.sedp.registry(),
                    ) {
                        return Ok(Some(ty));
                    }
                }
                for (server, info) in &infos {
                    request_complete_types(inner, *server, info);
                }
            }
            self.shared.flush();
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// How many TypeObjects of remote types this participant got from
    /// other participants with the TypeLookup service (DDS-XTypes 1.3,
    /// 7.6.3.3). It asks for them when an endpoint announces a type whose
    /// TypeObject it does not have, to check assignability.
    pub fn learnt_types(&self) -> usize {
        self.shared.lock_any().learnt_types
    }

    /// Creates a topic of type `T`, named `name`.
    pub fn create_topic<T: TopicType + 'static>(&self, name: &str) -> Result<Topic<T>> {
        Ok(Topic::new(
            name,
            self.shared.clone(),
            Arc::new(Typed::<T>::new()),
        ))
    }

    /// Creates a content filtered topic named `name` (DDS 1.4,
    /// 2.2.2.2.1.7): its readers get the samples of `related` that pass
    /// the filter `filter_expression` (DDS 1.4, Annex B; see
    /// [`ContentFilter`]) with `expression_parameters` for its `%n`.
    ///
    /// The filter reads the fields of the samples by their names in the
    /// type's description, so the type must be registered: a dynamic
    /// topic's is, and a [`TopicType`] needs
    /// [`register_type`](Self::register_type) first, or this fails with
    /// [`Error::PreconditionNotMet`]. A filter expression that is not
    /// valid for the type fails with [`Error::Dynamic`].
    pub fn create_contentfilteredtopic<T>(
        &self,
        name: &str,
        related: &Topic<T>,
        filter_expression: &str,
        expression_parameters: &[String],
    ) -> Result<ContentFilteredTopic<T>> {
        let ty = self
            .shared
            .dynamic_type(related.type_name())?
            .ok_or(Error::PreconditionNotMet)?;
        let filter = ContentFilter::new(&ty, filter_expression, expression_parameters)?;
        Ok(ContentFilteredTopic::new(name, related, filter))
    }

    /// Creates a publisher in the default partition.
    pub fn create_publisher(&self) -> Result<Publisher> {
        self.create_publisher_with_qos(PublisherQos::default())
    }

    /// Creates a publisher with the given QoS (its partitions).
    pub fn create_publisher_with_qos(&self, qos: PublisherQos) -> Result<Publisher> {
        Ok(Publisher::new(
            self.shared.clone(),
            PublisherEntity::new(qos),
        ))
    }

    /// Creates a subscriber in the default partition.
    pub fn create_subscriber(&self) -> Result<Subscriber> {
        self.create_subscriber_with_qos(SubscriberQos::default())
    }

    /// Creates a subscriber with the given QoS (its partitions).
    pub fn create_subscriber_with_qos(&self, qos: SubscriberQos) -> Result<Subscriber> {
        Ok(Subscriber::from_entity(
            self.shared.clone(),
            SubscriberEntity::new(qos),
        ))
    }

    /// Sets the participant's listener (`set_listener`, DDS 1.4,
    /// 2.2.2.1.1), replacing any earlier one: it is called for the
    /// statuses in `mask` of its entities that their own listeners, and
    /// those of their publishers, subscribers or topics, do not take
    /// (2.2.4.3.1). See [`DomainParticipantListener`].
    pub fn set_listener(
        &self,
        listener: impl DomainParticipantListener + 'static,
        mask: StatusMask,
    ) {
        self.shared.lock_any().listener = Some((Arc::new(listener), mask));
    }

    /// Removes the participant's listener.
    pub fn clear_listener(&self) {
        self.shared.lock_any().listener = None;
    }

    /// Asserts the liveliness of the participant's MANUAL_BY_PARTICIPANT
    /// writers (`assert_liveliness`, DDS 1.4, 2.2.2.2.1.19). Writing with
    /// any of them does too.
    pub fn assert_liveliness(&self) -> Result<()> {
        self.shared.lock()?.participant_asserted(Instant::now());
        self.shared.flush();
        Ok(())
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
        // The authenticated participants take the deletion only from
        // DCPSParticipantsSecure (DDS Security 1.2, 7.5.1.6): it is
        // written there, and the thread is given a moment to deliver it.
        let disposed = {
            let mut g = self.shared.lock_any();
            g.spdp_secure.as_mut().map(SpdpSecure::dispose)
        };
        if let Some(sn) = disposed {
            let deadline = Instant::now() + SECURE_DISPOSE_WAIT;
            while Instant::now() < deadline
                && !self
                    .shared
                    .lock_any()
                    .spdp_secure
                    .as_ref()
                    .is_none_or(|s| s.is_acked_by_all(sn))
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let mut g = self.shared.lock_any();
        g.deleted = true;
        // Tell the others at once, rather than letting the lease run out
        // (8.5.3.1).
        let (bytes, to) = g.spdp.dispose();
        let to = to.to_vec();
        drop(g);
        let _ = self.shared.send(&bytes, &to);
        self.shared.data_arrived.notify_all();
        self.shared.acknowledged.notify_all();
        self.shared.wake_waitsets();
    }
}

/// How long deleting a participant waits for its dispose on
/// DCPSParticipantsSecure to be acknowledged.
const SECURE_DISPOSE_WAIT: Duration = Duration::from_millis(300);

/// The longest the thread sleeps, so that it sees the stop flag.
const MAX_WAIT: Duration = Duration::from_millis(50);

fn run(shared: Arc<Shared>, incoming: UdpIncoming) {
    while !shared.stop.load(Ordering::Relaxed) {
        let wait = {
            let g = shared.lock_any();
            let manual = g.manual_pending.then_some(g.next_manual);
            let next = [
                g.spdp.next_deadline(),
                g.sedp.next_deadline(),
                g.wlp.next_deadline(),
                g.wlp_secure.as_ref().and_then(Wlp::next_deadline),
                g.spdp_secure.as_ref().and_then(SpdpSecure::next_deadline),
                g.typelookup.next_deadline(),
                g.next_automatic,
                manual,
                #[cfg(feature = "security")]
                g.security.as_ref().and_then(|s| s.next_deadline()),
            ]
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
    let mut out: Vec<(Vec<u8>, Vec<Locator>)> = Vec::new();
    {
        let mut g = shared.lock_any();
        let inner = &mut *g;
        let mut acked = false;
        for d in datagrams {
            #[cfg(feature = "security")]
            let d = match &inner.security {
                Some(sec) => match sec.decode(&d.data) {
                    Some(b) => {
                        let b = b.into_owned();
                        if matches!(
                            d.channel,
                            Channel::DiscoveryMulticast | Channel::DiscoveryUnicast
                        ) {
                            sec.handle_discovery(&b, now);
                        }
                        Datagram { data: b, ..d }
                    }
                    None => continue,
                },
                None => d,
            };
            match d.channel {
                Channel::DiscoveryMulticast | Channel::DiscoveryUnicast => {
                    for e in inner.spdp.handle_message(&d.data, now) {
                        participant_event(inner, e, now);
                    }
                    let events = inner.sedp.handle_message(&d.data, now);
                    apply_sedp_events(inner, events, now);
                    for a in inner.wlp.handle_message(&d.data, now) {
                        inner.remote_participant_asserted(a.participant, a.kind, now);
                    }
                    if let Some(w) = inner.wlp_secure.as_mut() {
                        for a in w.handle_message(&d.data, now) {
                            inner.remote_participant_asserted(a.participant, a.kind, now);
                        }
                    }
                    if let Some(s) = inner.spdp_secure.as_mut() {
                        for e in s.handle_message(&d.data, now) {
                            if let Some(e) = inner.spdp.handle_secure(e, now) {
                                participant_event(inner, e, now);
                            }
                        }
                    }
                    let replies =
                        inner
                            .typelookup
                            .handle_message(&d.data, now, inner.sedp.registry());
                    for r in replies {
                        type_lookup_replied(inner, r, now);
                    }
                }
                Channel::UserMulticast | Channel::UserUnicast => {
                    if shared.should_drop() {
                        continue;
                    }
                    acked |= user_traffic(shared.prefix, inner, &d.data, now);
                }
            }
        }
        for e in inner.spdp.remove_expired(now) {
            participant_event(inner, e, now);
        }
        inner.check_timers(now);
        if let Some((bytes, to)) = inner.spdp.poll_announce(now) {
            out.push((bytes, to.to_vec()));
        }
        for o in inner.sedp.poll(now) {
            out.push((o.bytes, o.locators));
        }
        for o in inner.wlp.poll(now) {
            out.push((o.bytes, o.locators));
        }
        if let Some(w) = inner.wlp_secure.as_mut() {
            for o in w.poll(now) {
                out.push((o.bytes, o.locators));
            }
        }
        if let Some(s) = inner.spdp_secure.as_mut() {
            for o in s.poll(now) {
                out.push((o.bytes, o.locators));
            }
        }
        for o in inner.typelookup.poll(now) {
            out.push((o.bytes, o.locators));
        }
        #[cfg(feature = "security")]
        if let Some(sec) = inner.security.clone() {
            let (o, events) = sec.poll(now);
            out.extend(o.into_iter().map(|o| (o.bytes, o.locators)));
            for e in events {
                match e {
                    PeerEvent::Ready(p) => participant_ready(inner, &p, now),
                }
            }
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
        if acked {
            shared.acknowledged.notify_all();
        }
    }
    for (bytes, to) in out {
        // Discovery and reliable traffic are repeated, so a datagram
        // that cannot be sent now is sent again later.
        let _ = shared.send(&bytes, &to);
    }
    shared.flush();
}

fn participant_event(inner: &mut Inner, e: ParticipantEvent, now: Instant) {
    match e {
        ParticipantEvent::Discovered(p) | ParticipantEvent::Updated(p) => {
            #[cfg(feature = "security")]
            if let Some(sec) = &inner.security {
                if !sec.participant_discovered(&p, now) {
                    return;
                }
            }
            participant_ready(inner, &p, now);
        }
        ParticipantEvent::Removed(prefix, _) => {
            #[cfg(feature = "security")]
            if let Some(sec) = &inner.security {
                sec.participant_removed(&prefix);
            }
            inner.wlp.remove_participant(&prefix);
            if let Some(w) = inner.wlp_secure.as_mut() {
                w.remove_participant(&prefix);
            }
            if let Some(s) = inner.spdp_secure.as_mut() {
                s.remove_participant(&prefix);
            }
            inner.typelookup.remove_participant(&prefix);
            inner.type_requests.retain(|(_, p)| *p != prefix);
            let events = inner.sedp.remove_participant(&prefix);
            apply_sedp_events(inner, events, now);
        }
    }
}

/// Discovery goes on with a participant (after authentication, with
/// security).
fn participant_ready(inner: &mut Inner, p: &ParticipantData, now: Instant) {
    inner.sedp.add_participant(p, now);
    inner.wlp.add_participant(p, now);
    if let Some(w) = inner.wlp_secure.as_mut() {
        w.add_participant(p, now);
    }
    if let Some(s) = inner.spdp_secure.as_mut() {
        s.add_participant(p, now);
    }
    inner.typelookup.add_participant(p, now);
}

/// Handles a user-traffic message: DATA, DATA_FRAG, GAP, HEARTBEAT and
/// HEARTBEAT_FRAG for the local readers, ACKNACK and NACK_FRAG for the
/// local writers. Returns whether some writer got an ACKNACK.
fn user_traffic(local: GuidPrefix, inner: &mut Inner, bytes: &[u8], now: Instant) -> bool {
    let mut acked = false;
    let Some((_, received)) = receive(bytes, local) else {
        return false;
    };
    for r in received {
        let src = r.source_prefix;
        match r.submessage {
            Submessage::AckNack(a) => {
                if let Some(w) = inner.writers.get_mut(&Guid::new(local, a.writer_id)) {
                    w.rtps.handle_acknack(Guid::new(src, a.reader_id), &a, now);
                    w.trim();
                    acked = true;
                }
            }
            Submessage::Data(d) => {
                let writer = Guid::new(src, d.writer_id);
                inner.remote_writer_asserted(&writer, now);
                for_readers(inner, d.reader_id, |st| match st.rtps.as_mut() {
                    Some(rtps) => rtps.handle_data(writer, &d, r.timestamp),
                    None => vec![Change {
                        writer,
                        sequence_number: d.writer_sn,
                        inline_qos: d.inline_qos.clone(),
                        payload: d.serialized_payload.clone(),
                        timestamp: r.timestamp,
                    }],
                });
            }
            Submessage::DataFrag(d) => {
                let writer = Guid::new(src, d.writer_id);
                inner.remote_writer_asserted(&writer, now);
                for_readers(inner, d.reader_id, |st| match st.rtps.as_mut() {
                    Some(rtps) => rtps.handle_data_frag(writer, &d, r.timestamp),
                    // Best effort: the newest incomplete samples are
                    // kept, and only those of matched writers.
                    None if st.matched.contains(&writer) => {
                        let limits = st.reassembly;
                        st.fragments
                            .entry(writer)
                            .or_insert_with(|| Reassembler::new(limits, Keep::Highest))
                            .add(writer, &d, r.timestamp)
                            .into_iter()
                            .collect()
                    }
                    None => Vec::new(),
                });
            }
            Submessage::HeartbeatFrag(h) => {
                let writer = Guid::new(src, h.writer_id);
                for_readers(inner, h.reader_id, |st| {
                    if let Some(rtps) = st.rtps.as_mut() {
                        rtps.handle_heartbeat_frag(writer, &h, now);
                    }
                    Vec::new()
                });
            }
            Submessage::NackFrag(n) => {
                if let Some(w) = inner.writers.get_mut(&Guid::new(local, n.writer_id)) {
                    w.rtps.handle_nackfrag(Guid::new(src, n.reader_id), &n, now);
                }
            }
            Submessage::Gap(gap) => {
                let writer = Guid::new(src, gap.writer_id);
                for_readers(inner, gap.reader_id, |st| match st.rtps.as_mut() {
                    Some(rtps) => rtps.handle_gap(writer, &gap),
                    None => Vec::new(),
                });
            }
            Submessage::Heartbeat(h) => {
                let writer = Guid::new(src, h.writer_id);
                if h.liveliness_flag {
                    // MANUAL_BY_TOPIC assert_liveliness (8.7.2.2.3).
                    inner.remote_writer_asserted(&writer, now);
                }
                for_readers(inner, h.reader_id, |st| match st.rtps.as_mut() {
                    Some(rtps) => rtps.handle_heartbeat(writer, &h, now),
                    None => Vec::new(),
                });
            }
            _ => {}
        }
    }
    acked
}

/// Runs `f` on the local readers a submessage is for (`reader_id`, or
/// every reader when it is `ENTITYID_UNKNOWN`) and keeps the changes it
/// returns.
fn for_readers(
    inner: &mut Inner,
    reader_id: EntityId,
    mut f: impl FnMut(&mut ReaderState) -> Vec<Change>,
) {
    for (rg, st) in inner.readers.iter_mut() {
        if reader_id != EntityId::UNKNOWN && reader_id != rg.entity_id {
            continue;
        }
        for c in f(st) {
            let Some(c) = st.incoming(c) else {
                continue;
            };
            let k = st.accept(c);
            if !k.is_empty() {
                inner.pending.push((*rg, k));
            }
        }
    }
}

#[cfg(test)]
mod padding_tests {
    use super::drop_missing_padding;

    #[test]
    fn padding_counted_but_not_sent_is_dropped() {
        // OpenDDS: "Hello, world!" (22 octets of data), 2 counted, none sent.
        let mut p = vec![0, 1, 0, 2, 1, 0, 0, 0, 14, 0, 0, 0];
        p.extend_from_slice(b"Hello, world!\0");
        drop_missing_padding(&mut p);
        assert_eq!(p[3], 0);
        // The same with the 2 octets sent: kept as it is.
        let mut q = vec![0, 1, 0, 2, 1, 0, 0, 0, 14, 0, 0, 0];
        q.extend_from_slice(b"Hello, world!\0\0\0");
        drop_missing_padding(&mut q);
        assert_eq!(q[3], 2);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;

    #[test]
    fn properties_without_dds_sec_are_ignored() {
        let mut c = ParticipantConfig::new(0);
        c.set_properties([("some.vendor.thing", "1")]).unwrap();
        #[cfg(feature = "security")]
        assert!(c.security.is_none());
    }

    #[cfg(not(feature = "security"))]
    #[test]
    fn dds_sec_properties_need_the_feature() {
        let mut c = ParticipantConfig::new(0);
        let e = c
            .set_properties([("dds.sec.auth.identity_ca", "file:ca.pem")])
            .unwrap_err();
        assert!(matches!(e, Error::Unsupported(_)));
    }

    #[cfg(feature = "security")]
    #[test]
    fn dds_sec_properties_make_the_security_config() {
        let mut c = ParticipantConfig::new(0);
        let e = c
            .set_properties([("dds.sec.auth.identity_ca", "file:ca.pem")])
            .unwrap_err();
        assert!(e.to_string().contains("dds.sec.access.permissions"), "{e}");
        let props: Vec<(&str, String)> = SecurityConfig::PROPERTIES
            .iter()
            .map(|n| (*n, format!("file:{n}")))
            .collect();
        c.set_properties(props.iter().map(|(n, v)| (*n, v.as_str())))
            .unwrap();
        let s = c.security.unwrap();
        assert_eq!(s.identity_ca, "file:dds.sec.auth.identity_ca");
        assert_eq!(s.permissions, "file:dds.sec.access.permissions");
    }
}
