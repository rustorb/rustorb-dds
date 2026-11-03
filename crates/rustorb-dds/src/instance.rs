//! Instances and the history of a data reader (DDS 1.4, 2.2.2.5.1): the
//! samples kept, the state of each instance, and what read and take
//! return.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ops::BitOr;

use rustorb_rtps::{Guid, SequenceNumber, Time};

use crate::qos::{History, ResourceLimits};
use crate::status::{SampleLostStatus, SampleRejectedReason, SampleRejectedStatus};

/// Identifies an instance: its key hash (DDSI-RTPS 2.5, 9.6.4.8). Every
/// writer and reader of a topic gives an instance the same handle. A type
/// without key has one instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceHandle(pub [u8; 16]);

impl InstanceHandle {
    /// The one instance of a type without key.
    pub const KEYLESS: InstanceHandle = InstanceHandle([0; 16]);
}

/// Whether a sample was read before (DDS 1.4, 2.2.2.5.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleState {
    /// Returned by an earlier `read`.
    Read,
    /// Not returned before.
    NotRead,
}

/// Whether the reader has seen the instance before (2.2.2.5.1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewState {
    /// First accessed now, or reborn since it was last accessed.
    New,
    /// Accessed before, and not reborn since.
    NotNew,
}

/// The state of an instance (2.2.2.5.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstanceState {
    /// Written by live writers and not disposed.
    Alive,
    /// Disposed by a writer.
    NotAliveDisposed,
    /// No live writer writes it any more.
    NotAliveNoWriters,
}

macro_rules! mask {
    ($(#[$m:meta])* $name:ident, $state:ty, { $($(#[$c:meta])* $const:ident = $bits:expr, $variant:path;)* } any = $any:expr) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(u8);

        impl $name {
            $($(#[$c])* pub const $const: $name = $name($bits);)*
            /// Every state.
            pub const ANY: $name = $name($any);

            /// Whether the mask has `s`.
            pub fn contains(self, s: $state) -> bool {
                let bit = match s { $($variant => $bits,)* };
                self.0 & bit != 0
            }
        }

        impl BitOr for $name {
            type Output = $name;
            fn bitor(self, o: $name) -> $name {
                $name(self.0 | o.0)
            }
        }

        impl Default for $name {
            /// [`Self::ANY`].
            fn default() -> Self {
                Self::ANY
            }
        }
    };
}

mask!(
    /// A set of [`SampleState`]s (`SampleStateMask`).
    SampleStateMask, SampleState, {
        /// `READ_SAMPLE_STATE`.
        READ = 1, SampleState::Read;
        /// `NOT_READ_SAMPLE_STATE`.
        NOT_READ = 2, SampleState::NotRead;
    } any = 3
);

mask!(
    /// A set of [`ViewState`]s (`ViewStateMask`).
    ViewStateMask, ViewState, {
        /// `NEW_VIEW_STATE`.
        NEW = 1, ViewState::New;
        /// `NOT_NEW_VIEW_STATE`.
        NOT_NEW = 2, ViewState::NotNew;
    } any = 3
);

mask!(
    /// A set of [`InstanceState`]s (`InstanceStateMask`).
    InstanceStateMask, InstanceState, {
        /// `ALIVE_INSTANCE_STATE`.
        ALIVE = 1, InstanceState::Alive;
        /// `NOT_ALIVE_DISPOSED_INSTANCE_STATE`.
        NOT_ALIVE_DISPOSED = 2, InstanceState::NotAliveDisposed;
        /// `NOT_ALIVE_NO_WRITERS_INSTANCE_STATE`.
        NOT_ALIVE_NO_WRITERS = 4, InstanceState::NotAliveNoWriters;
    } any = 7
);

impl InstanceStateMask {
    /// `NOT_ALIVE_INSTANCE_STATE`: disposed or without writers.
    pub const NOT_ALIVE: InstanceStateMask = InstanceStateMask(6);
}

/// Which samples a read or take returns: those whose states are in all
/// three masks. The default takes any.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StateFilter {
    /// The sample states.
    pub sample_states: SampleStateMask,
    /// The view states of the instances.
    pub view_states: ViewStateMask,
    /// The states of the instances.
    pub instance_states: InstanceStateMask,
}

impl StateFilter {
    /// Any sample.
    pub const ANY: StateFilter = StateFilter {
        sample_states: SampleStateMask::ANY,
        view_states: ViewStateMask::ANY,
        instance_states: InstanceStateMask::ANY,
    };

    /// The samples not read yet.
    pub const NOT_READ: StateFilter = StateFilter {
        sample_states: SampleStateMask::NOT_READ,
        view_states: ViewStateMask::ANY,
        instance_states: InstanceStateMask::ANY,
    };

    fn accepts(&self, s: SampleState, v: ViewState, i: InstanceState) -> bool {
        self.sample_states.contains(s)
            && self.view_states.contains(v)
            && self.instance_states.contains(i)
    }
}

/// Information about a sample (DDS 1.4 `SampleInfo`, 2.2.2.5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleInfo {
    /// Whether it was read before.
    pub sample_state: SampleState,
    /// The view state of its instance when the read or take was called.
    pub view_state: ViewState,
    /// The state of its instance when the read or take was called.
    pub instance_state: InstanceState,
    /// Whether the sample has data. Samples without data tell of a
    /// change in the state of the instance (2.2.2.5.1.4).
    pub valid_data: bool,
    /// When the writer wrote it, if the writer said
    /// (`source_timestamp`).
    pub source_timestamp: Option<Time>,
    /// Its instance.
    pub instance_handle: InstanceHandle,
    /// The writer that sent it (`publication_handle`).
    pub writer: Guid,
    /// Its sequence number from that writer.
    pub sequence_number: SequenceNumber,
    /// How many times the instance went from disposed to alive, when the
    /// sample arrived.
    pub disposed_generation_count: u32,
    /// How many times the instance went from no writers to alive, when
    /// the sample arrived.
    pub no_writers_generation_count: u32,
    /// How many samples of the same instance follow this one in the
    /// returned collection.
    pub sample_rank: u32,
    /// Generations between this sample and the most recent one of its
    /// instance in the collection.
    pub generation_rank: u32,
    /// Generations between this sample and the most recent one of its
    /// instance received.
    pub absolute_generation_rank: u32,
}

/// What a writer did to an instance, as a reader receives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// A value (a serialized payload).
    Write(Vec<u8>),
    /// `dispose`, `unregister`, or both (`PID_STATUS_INFO`, DDSI-RTPS
    /// 2.5, 9.6.4.9).
    Lifecycle { disposed: bool, unregistered: bool },
}

/// A change received from a writer, with its instance.
#[derive(Clone, Debug)]
pub(crate) struct Incoming {
    pub(crate) writer: Guid,
    pub(crate) sequence_number: SequenceNumber,
    pub(crate) source_timestamp: Option<Time>,
    pub(crate) instance: InstanceHandle,
    pub(crate) change: Change,
}

#[derive(Debug)]
struct Stored {
    /// The payload, `None` for a sample without data.
    payload: Option<Vec<u8>>,
    writer: Guid,
    sequence_number: SequenceNumber,
    source_timestamp: Option<Time>,
    read: bool,
    disposed_generation_count: u32,
    no_writers_generation_count: u32,
    /// The order of arrival in the reader.
    arrival: u64,
}

impl Stored {
    fn generation(&self) -> u32 {
        self.disposed_generation_count + self.no_writers_generation_count
    }
}

#[derive(Debug)]
struct Instance {
    state: InstanceState,
    /// NEW until a read or take returns a sample of it.
    new: bool,
    disposed_generation_count: u32,
    no_writers_generation_count: u32,
    /// The live writers that write it.
    writers: BTreeSet<Guid>,
    samples: VecDeque<Stored>,
    /// The last value received, for `get_key_value` and for recognising
    /// key-only payloads without a key hash.
    last_value: Option<Vec<u8>>,
}

impl Instance {
    fn new() -> Self {
        Instance {
            state: InstanceState::Alive,
            new: true,
            disposed_generation_count: 0,
            no_writers_generation_count: 0,
            writers: BTreeSet::new(),
            samples: VecDeque::new(),
            last_value: None,
        }
    }

    fn generation(&self) -> u32 {
        self.disposed_generation_count + self.no_writers_generation_count
    }
}

/// A sample returned by [`ReaderHistory::select`]: the payload (`None`
/// without data) and its information.
pub(crate) type Selected = (Option<Vec<u8>>, SampleInfo);

/// The samples and instances of one reader.
#[derive(Debug)]
pub(crate) struct ReaderHistory {
    history: History,
    limits: ResourceLimits,
    rejected: SampleRejectedStatus,
    /// SAMPLE_LOST.
    pub(crate) lost: SampleLostStatus,
    instances: BTreeMap<InstanceHandle, Instance>,
    /// The highest sequence number taken in from each writer: older ones
    /// are dropped (a best-effort reader keeps each writer's order,
    /// DDSI-RTPS 2.5, 8.4.11).
    last_sn: HashMap<Guid, SequenceNumber>,
    arrivals: u64,
}

impl ReaderHistory {
    #[cfg(test)]
    pub(crate) fn new(history: History) -> Self {
        Self::with_limits(history, ResourceLimits::default())
    }

    /// A history that holds at most what `limits` allows (DDS 1.4,
    /// 2.2.3.19): a sample beyond them is rejected.
    pub(crate) fn with_limits(history: History, limits: ResourceLimits) -> Self {
        ReaderHistory {
            history,
            limits,
            rejected: SampleRejectedStatus::default(),
            lost: SampleLostStatus::default(),
            instances: BTreeMap::new(),
            last_sn: HashMap::new(),
            arrivals: 0,
        }
    }

    /// The last value received for each instance, to recognise a key
    /// payload.
    pub(crate) fn last_values(&self) -> impl Iterator<Item = (InstanceHandle, &[u8])> {
        self.instances
            .iter()
            .filter_map(|(h, i)| i.last_value.as_deref().map(|v| (*h, v)))
    }

    /// The last value received for an instance.
    pub(crate) fn last_value(&self, h: &InstanceHandle) -> Option<&[u8]> {
        self.instances.get(h)?.last_value.as_deref()
    }

    /// Whether the reader has the instance.
    pub(crate) fn has_instance(&self, h: &InstanceHandle) -> bool {
        self.instances.contains_key(h)
    }

    /// Whether the reader has the instance and it is ALIVE.
    pub(crate) fn is_alive(&self, h: &InstanceHandle) -> bool {
        self.instances
            .get(h)
            .is_some_and(|i| i.state == InstanceState::Alive)
    }

    /// Whether some sample passes `filter` (the trigger of a
    /// ReadCondition, DDS 1.4, 2.2.4.5).
    pub(crate) fn has_matching(&self, filter: StateFilter) -> bool {
        self.instances.values().any(|i| {
            let view = if i.new {
                ViewState::New
            } else {
                ViewState::NotNew
            };
            i.samples.iter().any(|s| {
                let st = if s.read {
                    SampleState::Read
                } else {
                    SampleState::NotRead
                };
                filter.accepts(st, view, i.state)
            })
        })
    }

    /// The rejected count, to tell whether an `accept` rejected.
    pub(crate) fn rejected_count(&self) -> u32 {
        self.rejected.total_count
    }

    /// Whether some sample has not been read.
    pub(crate) fn has_unread(&self) -> bool {
        self.instances
            .values()
            .any(|i| i.samples.iter().any(|s| !s.read))
    }

    /// Takes a change in. Returns whether a sample was added.
    pub(crate) fn accept(&mut self, c: Incoming) -> bool {
        let first = !self.last_sn.contains_key(&c.writer);
        let last = self.last_sn.entry(c.writer).or_insert(SequenceNumber(0));
        if c.sequence_number <= *last {
            return false;
        }
        // Sequence numbers skipped are samples lost, except before the
        // first one taken from a writer: a reader matched late was never
        // meant to get those.
        if !first && c.sequence_number.0 > last.0 + 1 {
            let n = c.sequence_number.0 - last.0 - 1;
            self.lost.add(u32::try_from(n).unwrap_or(u32::MAX));
        }
        *last = c.sequence_number;
        if matches!(c.change, Change::Write(_)) {
            if let Some(reason) = self.rejects(&c.instance) {
                self.rejected.total_count += 1;
                self.rejected.total_count_change += 1;
                self.rejected.last_reason = Some(reason);
                self.rejected.last_instance_handle = Some(c.instance);
                return false;
            }
        }
        let inst = self
            .instances
            .entry(c.instance)
            .or_insert_with(Instance::new);
        let mut payload = None;
        match c.change {
            Change::Write(v) => {
                match inst.state {
                    InstanceState::Alive => {}
                    InstanceState::NotAliveDisposed => {
                        inst.disposed_generation_count += 1;
                        inst.new = true;
                    }
                    InstanceState::NotAliveNoWriters => {
                        inst.no_writers_generation_count += 1;
                        inst.new = true;
                    }
                }
                inst.state = InstanceState::Alive;
                inst.writers.insert(c.writer);
                inst.last_value = Some(v.clone());
                payload = Some(v);
            }
            Change::Lifecycle {
                disposed,
                unregistered,
            } => {
                let before = inst.state;
                if disposed {
                    inst.state = InstanceState::NotAliveDisposed;
                }
                if unregistered {
                    inst.writers.remove(&c.writer);
                    if inst.writers.is_empty() && inst.state == InstanceState::Alive {
                        inst.state = InstanceState::NotAliveNoWriters;
                    }
                }
                if inst.state == before {
                    self.reclaim(c.instance);
                    return false;
                }
            }
        }
        self.arrivals += 1;
        let inst = self.instances.get_mut(&c.instance).expect("added above");
        inst.samples.push_back(Stored {
            payload,
            writer: c.writer,
            sequence_number: c.sequence_number,
            source_timestamp: c.source_timestamp,
            read: false,
            disposed_generation_count: inst.disposed_generation_count,
            no_writers_generation_count: inst.no_writers_generation_count,
            arrival: self.arrivals,
        });
        if let History::KeepLast(depth) = self.history {
            while inst.samples.len() > depth.max(1) {
                inst.samples.pop_front();
            }
        }
        true
    }

    /// Why a new value of instance `h` would go beyond the resource
    /// limits, if it would. Under KEEP_LAST, a value that replaces the
    /// oldest of a full instance takes no more room.
    fn rejects(&self, h: &InstanceHandle) -> Option<SampleRejectedReason> {
        let l = self.limits;
        let inst = self.instances.get(h);
        if inst.is_none()
            && l.max_instances
                .is_some_and(|m| self.instances.len() >= m.max(1))
        {
            return Some(SampleRejectedReason::InstancesLimit);
        }
        let in_instance = inst.map_or(0, |i| i.samples.len());
        let replaces = matches!(self.history, History::KeepLast(d) if in_instance >= d.max(1));
        if replaces {
            return None;
        }
        if l.max_samples_per_instance
            .is_some_and(|m| in_instance >= m.max(1))
        {
            return Some(SampleRejectedReason::SamplesPerInstanceLimit);
        }
        let total: usize = self.instances.values().map(|i| i.samples.len()).sum();
        if l.max_samples.is_some_and(|m| total >= m.max(1)) {
            return Some(SampleRejectedReason::SamplesLimit);
        }
        None
    }

    /// The SAMPLE_REJECTED status; reading it resets the change count.
    pub(crate) fn take_rejected_status(&mut self) -> SampleRejectedStatus {
        let s = self.rejected;
        self.rejected.total_count_change = 0;
        s
    }

    /// A writer lost its liveliness (DDS 1.4, 2.2.3.11): its instances
    /// lose it as a writer, as in [`Self::writer_gone`], but what it sent
    /// before is still told apart from what it sends once it is back.
    pub(crate) fn writer_lost(&mut self, writer: &Guid) -> bool {
        let last = self.last_sn.get(writer).copied();
        let added = self.writer_gone(writer);
        if let Some(sn) = last {
            self.last_sn.insert(*writer, sn);
        }
        added
    }

    /// A writer is gone (unmatched, or deleted): the instances it wrote
    /// lose it, and those left without writers become
    /// NOT_ALIVE_NO_WRITERS. Returns whether a sample was added.
    pub(crate) fn writer_gone(&mut self, writer: &Guid) -> bool {
        self.last_sn.remove(writer);
        let handles: Vec<InstanceHandle> = self
            .instances
            .iter()
            .filter(|(_, i)| i.writers.contains(writer))
            .map(|(h, _)| *h)
            .collect();
        let mut added = false;
        for h in handles {
            let inst = self.instances.get_mut(&h).expect("listed above");
            inst.writers.remove(writer);
            if !inst.writers.is_empty() || inst.state != InstanceState::Alive {
                continue;
            }
            inst.state = InstanceState::NotAliveNoWriters;
            self.arrivals += 1;
            let s = Stored {
                payload: None,
                writer: *writer,
                sequence_number: SequenceNumber(0),
                source_timestamp: None,
                read: false,
                disposed_generation_count: inst.disposed_generation_count,
                no_writers_generation_count: inst.no_writers_generation_count,
                arrival: self.arrivals,
            };
            inst.samples.push_back(s);
            if let History::KeepLast(depth) = self.history {
                while inst.samples.len() > depth.max(1) {
                    inst.samples.pop_front();
                }
            }
            added = true;
        }
        added
    }

    /// Forgets an instance with no samples and no live writers
    /// (2.2.2.5.1.8).
    fn reclaim(&mut self, h: InstanceHandle) {
        if self
            .instances
            .get(&h)
            .is_some_and(|i| i.samples.is_empty() && i.writers.is_empty())
        {
            self.instances.remove(&h);
        }
    }

    /// The instance after `prev` (in handle order) that has samples
    /// passing `filter`.
    pub(crate) fn next_instance(
        &self,
        prev: Option<InstanceHandle>,
        filter: StateFilter,
    ) -> Option<InstanceHandle> {
        use std::ops::Bound;
        let from = match prev {
            Some(p) => Bound::Excluded(p),
            None => Bound::Unbounded,
        };
        self.instances
            .range((from, Bound::Unbounded))
            .find(|(_, i)| {
                let v = if i.new {
                    ViewState::New
                } else {
                    ViewState::NotNew
                };
                i.samples.iter().any(|s| {
                    let st = if s.read {
                        SampleState::Read
                    } else {
                        SampleState::NotRead
                    };
                    filter.accepts(st, v, i.state)
                })
            })
            .map(|(h, _)| *h)
    }

    /// Returns the samples passing `filter` (of one instance, if given),
    /// in the order they arrived, and marks them read; `take` removes
    /// them instead.
    pub(crate) fn select(
        &mut self,
        filter: StateFilter,
        instance: Option<InstanceHandle>,
        take: bool,
    ) -> Vec<Selected> {
        // Instances emptied by an earlier take are forgotten now, not at
        // that take, so that their key stays available until the next
        // read or take (for `get_key_value` on a sample without data).
        let empty: Vec<InstanceHandle> = self
            .instances
            .iter()
            .filter(|(_, i)| i.samples.is_empty() && i.writers.is_empty())
            .map(|(h, _)| *h)
            .collect();
        for h in empty {
            self.instances.remove(&h);
        }
        let mut picked: Vec<(u64, Option<Vec<u8>>, SampleInfo)> = Vec::new();
        let handles: Vec<InstanceHandle> = match instance {
            Some(h) => vec![h],
            None => self.instances.keys().copied().collect(),
        };
        for h in &handles {
            let Some(inst) = self.instances.get_mut(h) else {
                continue;
            };
            let view = if inst.new {
                ViewState::New
            } else {
                ViewState::NotNew
            };
            let state = inst.state;
            let current_generation = inst.generation();
            let mut kept = VecDeque::new();
            let mut mine = Vec::new();
            for s in inst.samples.drain(..) {
                let st = if s.read {
                    SampleState::Read
                } else {
                    SampleState::NotRead
                };
                if !filter.accepts(st, view, state) {
                    kept.push_back(s);
                    continue;
                }
                let info = SampleInfo {
                    sample_state: st,
                    view_state: view,
                    instance_state: state,
                    valid_data: s.payload.is_some(),
                    source_timestamp: s.source_timestamp,
                    instance_handle: *h,
                    writer: s.writer,
                    sequence_number: s.sequence_number,
                    disposed_generation_count: s.disposed_generation_count,
                    no_writers_generation_count: s.no_writers_generation_count,
                    sample_rank: 0,
                    generation_rank: 0,
                    absolute_generation_rank: current_generation - s.generation(),
                };
                if take {
                    mine.push((s.arrival, s.payload, info));
                } else {
                    mine.push((s.arrival, s.payload.clone(), info));
                    kept.push_back(Stored { read: true, ..s });
                }
            }
            inst.samples = kept;
            if !mine.is_empty() {
                inst.new = false;
                let n = mine.len();
                let last_generation = mine[n - 1].2.disposed_generation_count
                    + mine[n - 1].2.no_writers_generation_count;
                for (i, (_, _, info)) in mine.iter_mut().enumerate() {
                    info.sample_rank = (n - 1 - i) as u32;
                    info.generation_rank = last_generation
                        - (info.disposed_generation_count + info.no_writers_generation_count);
                }
            }
            picked.extend(mine);
        }
        picked.sort_by_key(|(a, _, _)| *a);
        picked.into_iter().map(|(_, p, i)| (p, i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustorb_rtps::{EntityId, GuidPrefix};

    fn w(n: u8) -> Guid {
        Guid::new(GuidPrefix([n; 12]), EntityId::user([0, 0, 1], 0x02))
    }

    fn h(n: u8) -> InstanceHandle {
        InstanceHandle([n; 16])
    }

    fn write(writer: Guid, sn: i64, inst: u8, v: u8) -> Incoming {
        Incoming {
            writer,
            sequence_number: SequenceNumber(sn),
            source_timestamp: None,
            instance: h(inst),
            change: Change::Write(vec![v]),
        }
    }

    fn life(writer: Guid, sn: i64, inst: u8, disposed: bool, unregistered: bool) -> Incoming {
        Incoming {
            change: Change::Lifecycle {
                disposed,
                unregistered,
            },
            ..write(writer, sn, inst, 0)
        }
    }

    fn values(s: &[Selected]) -> Vec<Option<u8>> {
        s.iter().map(|(p, _)| p.as_ref().map(|p| p[0])).collect()
    }

    #[test]
    fn keep_last_is_per_instance() {
        let mut r = ReaderHistory::new(History::KeepLast(2));
        for (sn, (inst, v)) in [(1, 1), (2, 2), (1, 3), (1, 4), (2, 5)].iter().enumerate() {
            assert!(r.accept(write(w(1), sn as i64 + 1, *inst, *v)));
        }
        // Instance 1 keeps 3 and 4, instance 2 keeps 2 and 5; in arrival
        // order.
        let s = r.select(StateFilter::ANY, None, false);
        assert_eq!(values(&s), [Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(s[1].1.sample_rank, 1);
        assert_eq!(s[2].1.sample_rank, 0);
        assert!(s.iter().all(|(_, i)| i.view_state == ViewState::New));
        // Read again: now READ and NOT_NEW.
        let s = r.select(StateFilter::ANY, Some(h(2)), false);
        assert_eq!(values(&s), [Some(2), Some(5)]);
        assert!(
            s.iter()
                .all(|(_, i)| i.sample_state == SampleState::Read
                    && i.view_state == ViewState::NotNew)
        );
        assert!(r.select(StateFilter::NOT_READ, None, false).is_empty());
        assert_eq!(r.next_instance(None, StateFilter::ANY), Some(h(1)));
        assert_eq!(r.next_instance(Some(h(1)), StateFilter::ANY), Some(h(2)));
        assert_eq!(r.next_instance(Some(h(2)), StateFilter::ANY), None);
    }

    #[test]
    fn dispose_unregister_and_rebirth() {
        let mut r = ReaderHistory::new(History::KeepAll);
        r.accept(write(w(1), 1, 1, 10));
        r.accept(write(w(2), 1, 1, 11));
        r.accept(life(w(1), 2, 1, true, false));
        // Disposing again changes nothing.
        assert!(!r.accept(life(w(2), 2, 1, true, false)));
        let s = r.select(StateFilter::ANY, None, true);
        assert_eq!(values(&s), [Some(10), Some(11), None]);
        assert!(
            s.iter()
                .all(|(_, i)| i.instance_state == InstanceState::NotAliveDisposed)
        );
        assert!(!s[2].1.valid_data);

        // Written again: alive, NEW, a new generation.
        r.accept(write(w(1), 3, 1, 12));
        let s = r.select(StateFilter::ANY, None, false);
        assert_eq!(s[0].1.instance_state, InstanceState::Alive);
        assert_eq!(s[0].1.view_state, ViewState::New);
        assert_eq!(s[0].1.disposed_generation_count, 1);

        // Both writers unregister: no writers.
        r.accept(life(w(1), 4, 1, false, true));
        let filter = StateFilter {
            instance_states: InstanceStateMask::NOT_ALIVE_NO_WRITERS,
            ..StateFilter::ANY
        };
        assert!(r.select(filter, None, false).is_empty());
        r.accept(life(w(2), 3, 1, false, true));
        let s = r.select(filter, None, true);
        assert_eq!(values(&s), [Some(12), None]);
        // Nothing left, no writers: the instance is forgotten at the next
        // read or take.
        assert_eq!(r.last_value(&h(1)), Some(&[12][..]));
        assert!(r.select(StateFilter::ANY, None, true).is_empty());
        assert!(r.last_value(&h(1)).is_none());
    }

    #[test]
    fn writers_going_away_and_ranks() {
        let mut r = ReaderHistory::new(History::KeepAll);
        r.accept(write(w(1), 1, 1, 1));
        r.accept(write(w(1), 2, 2, 2));
        r.accept(write(w(2), 1, 2, 3));
        assert!(r.writer_gone(&w(1)));
        // Instance 2 still has writer 2.
        let s = r.select(StateFilter::ANY, None, false);
        assert_eq!(values(&s), [Some(1), Some(2), Some(3), None]);
        assert_eq!(s[3].1.instance_handle, h(1));
        assert_eq!(s[3].1.instance_state, InstanceState::NotAliveNoWriters);
        assert_eq!(s[1].1.instance_state, InstanceState::Alive);
        // Instance 1 comes back: generation 1 for the new sample.
        r.accept(write(w(1), 5, 1, 4));
        let s = r.select(StateFilter::ANY, Some(h(1)), true);
        assert_eq!(values(&s), [Some(1), None, Some(4)]);
        let ranks: Vec<(u32, u32, u32)> = s
            .iter()
            .map(|(_, i)| (i.sample_rank, i.generation_rank, i.absolute_generation_rank))
            .collect();
        assert_eq!(ranks, [(2, 1, 1), (1, 1, 1), (0, 0, 0)]);
        // Old sequence numbers from a writer are dropped.
        assert!(!r.accept(write(w(1), 4, 1, 9)));
        assert!(!r.has_unread());
    }

    #[test]
    fn masks() {
        let m = InstanceStateMask::NOT_ALIVE;
        assert!(m.contains(InstanceState::NotAliveDisposed));
        assert!(m.contains(InstanceState::NotAliveNoWriters));
        assert!(!m.contains(InstanceState::Alive));
        assert_eq!(
            InstanceStateMask::ALIVE | InstanceStateMask::NOT_ALIVE,
            InstanceStateMask::ANY
        );
        assert!(SampleStateMask::default().contains(SampleState::Read));
    }

    #[test]
    fn resource_limits_reject() {
        let limits = ResourceLimits {
            max_samples: Some(3),
            max_instances: Some(2),
            max_samples_per_instance: Some(2),
        };
        let mut r = ReaderHistory::with_limits(History::KeepAll, limits);
        let mut sn = 0;
        let mut put = |r: &mut ReaderHistory, inst: u8| {
            sn += 1;
            r.accept(write(w(1), sn, inst, inst))
        };
        assert!(put(&mut r, 1));
        assert!(put(&mut r, 1));
        // Two per instance.
        assert!(!put(&mut r, 1));
        assert!(put(&mut r, 2));
        // Three in all.
        assert!(!put(&mut r, 2));
        // Two instances.
        assert!(!put(&mut r, 3));
        let s = r.take_rejected_status();
        assert_eq!(s.total_count, 3);
        assert_eq!(s.total_count_change, 3);
        assert_eq!(s.last_reason, Some(SampleRejectedReason::InstancesLimit));
        assert_eq!(r.take_rejected_status().total_count_change, 0);
        // KEEP_LAST replaces within an instance instead.
        let mut r = ReaderHistory::with_limits(History::KeepLast(1), limits);
        for _ in 0..5 {
            assert!(put(&mut r, 1));
        }
    }
}
