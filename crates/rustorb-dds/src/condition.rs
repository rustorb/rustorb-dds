//! Conditions and WaitSets (DDS 1.4, 2.2.2.1.6 to 2.2.2.1.9 and
//! 2.2.4.4): a thread waits on a WaitSet until one of the conditions
//! attached to it is true, without an async runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use rustorb_rtps::Guid;

use crate::error::{Error, Result};
use crate::instance::StateFilter;
use crate::participant::Shared;
use crate::status::StatusMask;

/// What a WaitSet sleeps on: woken when one of its conditions may have
/// changed.
#[derive(Debug, Default)]
pub(crate) struct Waker {
    /// Counts the wakes, so that a wake between the check of the
    /// conditions and the wait is not missed.
    generation: Mutex<u64>,
    cv: Condvar,
}

impl Waker {
    pub(crate) fn wake(&self) {
        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.cv.notify_all();
    }

    fn generation(&self) -> u64 {
        *self.generation.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits until a wake after `generation`, or `deadline`.
    fn wait(&self, generation: u64, deadline: Option<Instant>) {
        let mut g = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        while *g == generation {
            match deadline {
                None => g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return;
                    }
                    g = match self.cv.wait_timeout(g, d - now) {
                        Ok((g, _)) => g,
                        Err(e) => e.into_inner().0,
                    };
                }
            }
        }
    }
}

/// Adds `waker` to `list` unless it is there, dropping the WaitSets gone.
fn register(list: &Mutex<Vec<Weak<Waker>>>, waker: &Arc<Waker>) {
    let mut l = list.lock().unwrap_or_else(|e| e.into_inner());
    l.retain(|w| w.strong_count() > 0);
    if !l.iter().any(|w| w.as_ptr() == Arc::as_ptr(waker)) {
        l.push(Arc::downgrade(waker));
    }
}

#[derive(Debug, Default)]
struct GuardState {
    trigger: Mutex<bool>,
    waitsets: Mutex<Vec<Weak<Waker>>>,
}

/// A condition whose trigger value the application sets (DDS 1.4,
/// 2.2.2.1.8): to wake a thread waiting on a WaitSet.
///
/// Clones are the same condition.
#[derive(Clone, Debug, Default)]
pub struct GuardCondition {
    state: Arc<GuardState>,
}

impl GuardCondition {
    /// A new condition, not triggered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the trigger value (`set_trigger_value`), waking the WaitSets
    /// it is attached to.
    pub fn set_trigger_value(&self, value: bool) {
        *self.state.trigger.lock().unwrap_or_else(|e| e.into_inner()) = value;
        let l = self
            .state
            .waitsets
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for w in l.iter().filter_map(Weak::upgrade) {
            w.wake();
        }
    }

    /// The trigger value (`get_trigger_value`).
    pub fn trigger_value(&self) -> bool {
        *self.state.trigger.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The condition of an entity's communication statuses (DDS 1.4,
/// 2.2.2.1.9): true when one of its enabled statuses has changed since it
/// was last read (`get_status_changes() & get_enabled_statuses()`).
///
/// A status is read with its `*_status` getter; DATA_AVAILABLE by
/// reading or taking samples. A status given to a listener is read too.
/// Every StatusCondition of an entity is the same condition.
#[derive(Clone)]
pub struct StatusCondition {
    shared: Arc<Shared>,
    entity: Guid,
}

impl std::fmt::Debug for StatusCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusCondition")
            .field("entity", &self.entity)
            .finish()
    }
}

impl PartialEq for StatusCondition {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared) && self.entity == other.entity
    }
}

impl StatusCondition {
    pub(crate) fn new(shared: Arc<Shared>, entity: Guid) -> Self {
        StatusCondition { shared, entity }
    }

    /// The GUID of its entity (`get_entity`).
    pub fn entity(&self) -> Guid {
        self.entity
    }

    /// Sets the statuses the condition is triggered by
    /// (`set_enabled_statuses`). All are, at first.
    pub fn set_enabled_statuses(&self, mask: StatusMask) {
        {
            let mut g = self.shared.lock_any();
            if let Some(w) = g.writers.get_mut(&self.entity) {
                w.enabled = mask;
            } else if let Some(r) = g.readers.get_mut(&self.entity) {
                r.enabled = mask;
            }
        }
        self.shared.wake_waitsets();
    }

    /// The statuses the condition is triggered by
    /// (`get_enabled_statuses`).
    pub fn enabled_statuses(&self) -> StatusMask {
        let g = self.shared.lock_any();
        if let Some(w) = g.writers.get(&self.entity) {
            w.enabled
        } else if let Some(r) = g.readers.get(&self.entity) {
            r.enabled
        } else {
            StatusMask::NONE
        }
    }

    /// The trigger value (`get_trigger_value`).
    pub fn trigger_value(&self) -> bool {
        let g = self.shared.lock_any();
        if let Some(w) = g.writers.get(&self.entity) {
            w.changes.intersects(w.enabled)
        } else if let Some(r) = g.readers.get(&self.entity) {
            r.changes.intersects(r.enabled)
        } else {
            false
        }
    }
}

/// A condition on the samples of a reader (DDS 1.4, 2.2.2.5.8): true
/// while the reader has a sample whose states pass its filter. Made with
/// `DataReader::create_readcondition`; reading or taking with
/// `DataReader::read_w_condition` or `take_w_condition` selects the same
/// samples.
///
/// Clones are the same condition.
#[derive(Clone)]
pub struct ReadCondition {
    shared: Arc<Shared>,
    reader: Guid,
    filter: StateFilter,
    /// Its identity.
    id: Arc<()>,
}

impl std::fmt::Debug for ReadCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadCondition")
            .field("reader", &self.reader)
            .field("filter", &self.filter)
            .finish()
    }
}

impl PartialEq for ReadCondition {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.id, &other.id)
    }
}

impl ReadCondition {
    pub(crate) fn new(shared: Arc<Shared>, reader: Guid, filter: StateFilter) -> Self {
        ReadCondition {
            shared,
            reader,
            filter,
            id: Arc::new(()),
        }
    }

    /// The GUID of its reader (`get_datareader`).
    pub fn reader(&self) -> Guid {
        self.reader
    }

    /// The states of the samples it is true for (`get_sample_state_mask`,
    /// `get_view_state_mask` and `get_instance_state_mask`).
    pub fn filter(&self) -> StateFilter {
        self.filter
    }

    /// The trigger value (`get_trigger_value`).
    pub fn trigger_value(&self) -> bool {
        let g = self.shared.lock_any();
        g.readers
            .get(&self.reader)
            .is_some_and(|r| r.samples.has_matching(self.filter))
    }
}

/// A condition to attach to a [`WaitSet`] (DDS 1.4 `Condition`,
/// 2.2.2.1.6).
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// A [`GuardCondition`].
    Guard(GuardCondition),
    /// A [`StatusCondition`].
    Status(StatusCondition),
    /// A [`ReadCondition`].
    Read(ReadCondition),
}

impl PartialEq for GuardCondition {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Condition {
    /// The trigger value (`get_trigger_value`).
    pub fn trigger_value(&self) -> bool {
        match self {
            Condition::Guard(c) => c.trigger_value(),
            Condition::Status(c) => c.trigger_value(),
            Condition::Read(c) => c.trigger_value(),
        }
    }

    /// Has the WaitSet of `waker` woken when the condition may change.
    fn register(&self, waker: &Arc<Waker>) {
        match self {
            Condition::Guard(c) => register(&c.state.waitsets, waker),
            Condition::Status(c) => register(&c.shared.waitsets, waker),
            Condition::Read(c) => register(&c.shared.waitsets, waker),
        }
    }
}

macro_rules! from_condition {
    ($t:ident, $v:ident) => {
        impl From<$t> for Condition {
            fn from(c: $t) -> Condition {
                Condition::$v(c)
            }
        }

        impl From<&$t> for Condition {
            fn from(c: &$t) -> Condition {
                Condition::$v(c.clone())
            }
        }

        impl PartialEq<$t> for Condition {
            fn eq(&self, other: &$t) -> bool {
                matches!(self, Condition::$v(c) if c == other)
            }
        }
    };
}

from_condition!(GuardCondition, Guard);
from_condition!(StatusCondition, Status);
from_condition!(ReadCondition, Read);

/// A set of conditions a thread waits on (DDS 1.4, 2.2.2.1.7): `wait`
/// returns when one of them is true.
///
/// One thread at a time may wait on a WaitSet. It works with plain
/// threads; no async runtime is needed.
///
/// ```no_run
/// # fn f(reader: &rustorb_dds::DataReader<impl rustorb_typesupport::TopicType>)
/// #     -> rustorb_dds::Result<()> {
/// use std::time::Duration;
/// use rustorb_dds::{GuardCondition, StatusMask, WaitSet};
///
/// let status = reader.status_condition();
/// status.set_enabled_statuses(StatusMask::DATA_AVAILABLE);
/// let stop = GuardCondition::new();
/// let waitset = WaitSet::new();
/// waitset.attach_condition(&status)?;
/// waitset.attach_condition(&stop)?;
/// loop {
///     let active = waitset.wait(Duration::from_secs(10))?;
///     if active.iter().any(|c| *c == stop) {
///         break;
///     }
///     for s in reader.take() {
///         println!("{:?}", s.info);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct WaitSet {
    waker: Arc<Waker>,
    conditions: Mutex<Vec<Condition>>,
    waiting: AtomicBool,
}

/// Clears the waiting flag of a WaitSet when its `wait` returns.
struct Waiting<'a>(&'a AtomicBool);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl WaitSet {
    /// An empty WaitSet.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Condition>> {
        self.conditions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Attaches a condition (`attach_condition`); attaching one already
    /// attached does nothing. A thread waiting on the WaitSet checks it
    /// at once.
    pub fn attach_condition(&self, condition: impl Into<Condition>) -> Result<()> {
        let c = condition.into();
        {
            let mut l = self.lock();
            if l.contains(&c) {
                return Ok(());
            }
            c.register(&self.waker);
            l.push(c);
        }
        self.waker.wake();
        Ok(())
    }

    /// Detaches a condition (`detach_condition`). Fails with
    /// [`Error::PreconditionNotMet`] if it is not attached.
    pub fn detach_condition(&self, condition: impl Into<Condition>) -> Result<()> {
        let c = condition.into();
        let mut l = self.lock();
        let Some(i) = l.iter().position(|x| *x == c) else {
            return Err(Error::PreconditionNotMet);
        };
        l.remove(i);
        Ok(())
    }

    /// The conditions attached (`get_conditions`).
    pub fn conditions(&self) -> Vec<Condition> {
        self.lock().clone()
    }

    /// Waits until at least one attached condition is true, and returns
    /// those that are (`wait`). Returns at once if one is true already.
    ///
    /// Fails with [`Error::Timeout`] if `timeout` passes first, and with
    /// [`Error::PreconditionNotMet`] if another thread is waiting on the
    /// WaitSet. A `timeout` of [`Duration::MAX`] waits for ever.
    pub fn wait(&self, timeout: Duration) -> Result<Vec<Condition>> {
        if self.waiting.swap(true, Ordering::AcqRel) {
            return Err(Error::PreconditionNotMet);
        }
        let _waiting = Waiting(&self.waiting);
        let deadline = Instant::now().checked_add(timeout);
        loop {
            // The generation is read before the conditions are checked:
            // a change after the check wakes the wait below.
            let generation = self.waker.generation();
            let active: Vec<Condition> = self
                .conditions()
                .into_iter()
                .filter(Condition::trigger_value)
                .collect();
            if !active.is_empty() {
                return Ok(active);
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Err(Error::Timeout);
            }
            self.waker.wait(generation, deadline);
        }
    }
}
