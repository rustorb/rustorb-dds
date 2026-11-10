//! `Subscriber` and `DataReader`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, entity_kind};

use crate::condition::{ReadCondition, StatusCondition};
use crate::error::{Error, Result};
use crate::instance::{InstanceHandle, SampleInfo, Selected, StateFilter};
use crate::listener::DataReaderListener;
use crate::participant::{ReaderEvent, ReaderListener, ReaderState, Shared};
use crate::qos::{DataReaderQos, SubscriberQos, TypeConsistency, duration_to_rtps};
use crate::status::{
    DeadlineMissedStatus, IncompatibleQosStatus, LivelinessChangedStatus, MatchedStatus,
    SampleLostStatus, SampleRejectedStatus, StatusMask,
};
use crate::topic::TopicDescription;
use crate::typed::SampleType;

/// A DDS subscriber (DDS 1.4, 2.2.2.5.1): creates data readers.
pub struct Subscriber {
    shared: Arc<Shared>,
    qos: SubscriberQos,
}

impl std::fmt::Debug for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriber").finish_non_exhaustive()
    }
}

impl Subscriber {
    pub(crate) fn new(shared: Arc<Shared>, qos: SubscriberQos) -> Self {
        Subscriber { shared, qos }
    }

    /// The subscriber's QoS.
    pub fn qos(&self) -> &SubscriberQos {
        &self.qos
    }

    /// Creates a reader for `topic` with the default QoS: BEST_EFFORT,
    /// KEEP_LAST(1), VOLATILE. `topic` is a [`Topic`](crate::Topic) or a
    /// [`ContentFilteredTopic`](crate::ContentFilteredTopic).
    pub fn create_datareader<T>(&self, topic: &impl TopicDescription<T>) -> Result<DataReader<T>> {
        self.create_datareader_with_qos(topic, DataReaderQos::default())
    }

    /// Creates a reader for `topic`.
    ///
    /// A RELIABLE reader gets the samples of each writer in order and
    /// without gaps, asking for those lost on the way; it matches only
    /// RELIABLE writers. A BEST_EFFORT reader takes what arrives, in
    /// order, and matches any writer.
    pub fn create_datareader_with_qos<T>(
        &self,
        topic: &impl TopicDescription<T>,
        qos: DataReaderQos,
    ) -> Result<DataReader<T>> {
        self.create(topic, qos, None)
    }

    /// Creates a reader for `topic` with a listener for the statuses in
    /// `mask`, set before the reader is matched, so that it is told of
    /// the writers already there and of the samples they kept
    /// (DDS 1.4, 2.2.2.5.2.5).
    pub fn create_datareader_with_listener<T: 'static>(
        &self,
        topic: &impl TopicDescription<T>,
        qos: DataReaderQos,
        listener: impl DataReaderListener<T> + 'static,
        mask: StatusMask,
    ) -> Result<DataReader<T>> {
        let l = reader_listener(topic.topic().samples.clone(), listener);
        self.create(topic, qos, Some((l, mask)))
    }

    fn create<T>(
        &self,
        description: &impl TopicDescription<T>,
        qos: DataReaderQos,
        listener: Option<(ReaderListener, StatusMask)>,
    ) -> Result<DataReader<T>> {
        let topic = description.topic();
        let cft = description.content_filter();
        let kind = if topic.samples.keyed() {
            entity_kind::USER_READER_WITH_KEY
        } else {
            entity_kind::USER_READER_NO_KEY
        };
        let guid = self.shared.new_guid(kind)?;
        let type_name = topic.type_name();
        let mut data = EndpointData::reader(guid, topic.name(), type_name);
        data.type_information = self.shared.type_information(type_name)?;
        data.data_representation = qos.data_representation.clone();
        if qos.type_consistency != TypeConsistency::default() {
            data.type_consistency = Some(qos.type_consistency);
        }
        data.reliability = qos.reliability.to_rtps();
        data.durability = qos.durability;
        data.deadline = duration_to_rtps(qos.deadline);
        data.liveliness = qos.liveliness.to_rtps();
        data.user_data = qos.user_data.clone();
        data.partitions = self.qos.partition.clone();
        let filter = cft.map(|c| {
            let (filter, property) = c.reader_filter();
            data.content_filter = Some(property);
            filter
        });
        self.shared
            .add_reader(data, qos, topic.samples.key_ops(), listener, filter)?;
        if let Some(c) = cft {
            c.add_reader(guid);
        }
        Ok(DataReader {
            shared: self.shared.clone(),
            guid,
            owned: true,
            samples: topic.samples.clone(),
        })
    }
}

/// A sample with its information.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample<T> {
    /// The value; `None` for a sample without data
    /// (`info.valid_data == false`), which only tells of a change in the
    /// state of its instance.
    pub data: Option<T>,
    /// The information.
    pub info: SampleInfo,
}

/// A DDS data reader (DDS 1.4, 2.2.2.5.3) of samples of type `T`: a
/// [`TopicType`](crate::TopicType), or [`DynamicData`](crate::DynamicData)
/// for a topic of a type known at run time (a `DynamicDataReader`,
/// DDS-XTypes 1.3, 7.5.2.12).
///
/// Samples that cannot be deserialized as `T` are dropped when they are
/// read or taken. Dropping the reader tells the other participants it is
/// gone.
pub struct DataReader<T> {
    pub(crate) shared: Arc<Shared>,
    pub(crate) guid: Guid,
    /// Whether dropping this handle deletes the reader: not for the
    /// handle given to a listener.
    owned: bool,
    samples: Arc<dyn SampleType<T>>,
}

/// A [`DataReaderListener`] for DATA_AVAILABLE alone, from a function.
struct DataAvailable<F>(F);

impl<T, F: Fn(&DataReader<T>) + Send + Sync> DataReaderListener<T> for DataAvailable<F> {
    fn on_data_available(&self, reader: &DataReader<T>) {
        (self.0)(reader)
    }
}

impl<T> std::fmt::Debug for DataReader<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataReader")
            .field("guid", &self.guid)
            .field("type_name", &self.samples.type_name())
            .finish()
    }
}

fn decode<T>(samples: &dyn SampleType<T>, (payload, info): Selected) -> Option<Sample<T>> {
    let data = match payload {
        Some(p) => Some(samples.decode(&p)?),
        None => None,
    };
    Some(Sample { data, info })
}

impl<T> DataReader<T> {
    /// The reader's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    fn select(
        &self,
        filter: StateFilter,
        instance: Option<InstanceHandle>,
        take: bool,
    ) -> Vec<Sample<T>> {
        let raw = {
            let mut g = self.shared.lock_any();
            match g.readers.get_mut(&self.guid) {
                Some(r) => {
                    r.changes = r.changes & !StatusMask::DATA_AVAILABLE;
                    r.samples.select(filter, instance, take)
                }
                None => Vec::new(),
            }
        };
        raw.into_iter()
            .filter_map(|r| decode(&*self.samples, r))
            .collect()
    }

    /// Removes and returns the samples received, in the order they
    /// arrived.
    pub fn take(&self) -> Vec<Sample<T>> {
        self.select(StateFilter::ANY, None, true)
    }

    /// Returns the samples received, in the order they arrived, and
    /// leaves them in the reader, marked as read.
    pub fn read(&self) -> Vec<Sample<T>> {
        self.select(StateFilter::ANY, None, false)
    }

    /// Like [`Self::take`], for the samples whose states pass `filter`.
    pub fn take_with(&self, filter: StateFilter) -> Vec<Sample<T>> {
        self.select(filter, None, true)
    }

    /// Like [`Self::read`], for the samples whose states pass `filter`.
    pub fn read_with(&self, filter: StateFilter) -> Vec<Sample<T>> {
        self.select(filter, None, false)
    }

    /// Takes the samples of one instance (`take_instance`).
    pub fn take_instance(&self, instance: InstanceHandle) -> Vec<Sample<T>> {
        self.select(StateFilter::ANY, Some(instance), true)
    }

    /// Reads the samples of one instance (`read_instance`).
    pub fn read_instance(&self, instance: InstanceHandle) -> Vec<Sample<T>> {
        self.select(StateFilter::ANY, Some(instance), false)
    }

    /// Takes the samples of the instance after `previous`, in handle
    /// order, that has samples passing `filter`; from the first instance
    /// when `previous` is `None` (`take_next_instance`). Empty when there
    /// is no such instance.
    pub fn take_next_instance(
        &self,
        previous: Option<InstanceHandle>,
        filter: StateFilter,
    ) -> Vec<Sample<T>> {
        self.next_instance(previous, filter, true)
    }

    /// Like [`Self::take_next_instance`], reading (`read_next_instance`).
    pub fn read_next_instance(
        &self,
        previous: Option<InstanceHandle>,
        filter: StateFilter,
    ) -> Vec<Sample<T>> {
        self.next_instance(previous, filter, false)
    }

    fn next_instance(
        &self,
        previous: Option<InstanceHandle>,
        filter: StateFilter,
        take: bool,
    ) -> Vec<Sample<T>> {
        let raw = {
            let mut g = self.shared.lock_any();
            let Some(r) = g.readers.get_mut(&self.guid) else {
                return Vec::new();
            };
            r.changes = r.changes & !StatusMask::DATA_AVAILABLE;
            match r.samples.next_instance(previous, filter) {
                Some(h) => r.samples.select(filter, Some(h), take),
                None => Vec::new(),
            }
        };
        raw.into_iter()
            .filter_map(|r| decode(&*self.samples, r))
            .collect()
    }

    /// The handle of the instance of `instance` (`lookup_instance`), if
    /// the reader has it.
    pub fn lookup_instance(&self, instance: &T) -> Option<InstanceHandle> {
        let h = self.samples.instance_of(instance).ok()?;
        let g = self.shared.lock_any();
        let r = g.readers.get(&self.guid)?;
        r.samples.has_instance(&h).then_some(h)
    }

    /// The key of an instance (`get_key_value`): the last value received
    /// for it, whose key members identify it. `None` if the reader has
    /// not received a value for it, or has forgotten the instance: one
    /// with no samples left and no writers is forgotten at the read or
    /// take after the one that emptied it.
    pub fn get_key_value(&self, instance: InstanceHandle) -> Option<T> {
        let g = self.shared.lock_any();
        let r = g.readers.get(&self.guid)?;
        self.samples.decode(r.samples.last_value(&instance)?)
    }

    /// Waits until there is a sample not yet read, or `timeout` passes.
    /// Returns whether there is one.
    pub fn wait_for_data(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.shared.lock_any();
        loop {
            let has = g
                .readers
                .get(&self.guid)
                .is_some_and(|r| r.samples.has_unread());
            if has {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            g = match self.shared.data_arrived.wait_timeout(g, deadline - now) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
        }
    }

    /// Sets the listener (`set_listener`), replacing any earlier one:
    /// its methods are called for the statuses in `mask` when they
    /// change. See [`DataReaderListener`].
    pub fn set_listener(&self, listener: impl DataReaderListener<T> + 'static, mask: StatusMask)
    where
        T: 'static,
    {
        let l = reader_listener(self.samples.clone(), listener);
        let mut g = self.shared.lock_any();
        if let Some(r) = g.readers.get_mut(&self.guid) {
            r.listener = Some((l, mask));
        }
    }

    /// Sets a listener for DATA_AVAILABLE alone: `f` is called with the
    /// reader when samples arrive. It may take them, or send to a
    /// channel:
    ///
    /// ```no_run
    /// # fn f<T: rustorb_typesupport::TopicType + Send + 'static>(
    /// #     reader: &rustorb_dds::DataReader<T>,
    /// # ) {
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// reader.on_data_available(move |r| {
    ///     for s in r.take() {
    ///         let _ = tx.send(s);
    ///     }
    /// });
    /// # }
    /// ```
    pub fn on_data_available(&self, f: impl Fn(&DataReader<T>) + Send + Sync + 'static)
    where
        T: 'static,
    {
        self.set_listener(DataAvailable(f), StatusMask::DATA_AVAILABLE);
    }

    /// Removes the listener.
    pub fn clear_listener(&self) {
        let mut g = self.shared.lock_any();
        if let Some(r) = g.readers.get_mut(&self.guid) {
            r.listener = None;
        }
    }

    /// The statuses changed since they were last read
    /// (`get_status_changes`).
    pub fn status_changes(&self) -> StatusMask {
        let g = self.shared.lock_any();
        g.readers
            .get(&self.guid)
            .map_or(StatusMask::NONE, |r| r.changes)
    }

    /// The reader's StatusCondition (`get_statuscondition`).
    pub fn status_condition(&self) -> StatusCondition {
        StatusCondition::new(self.shared.clone(), self.guid)
    }

    /// A ReadCondition that is true while the reader has samples whose
    /// states pass `filter` (`create_readcondition`). Dropping every
    /// clone of it deletes it.
    pub fn create_readcondition(&self, filter: StateFilter) -> ReadCondition {
        ReadCondition::new(self.shared.clone(), self.guid, filter)
    }

    fn check_condition(&self, condition: &ReadCondition) -> Result<StateFilter> {
        if condition.reader() != self.guid {
            return Err(Error::PreconditionNotMet);
        }
        Ok(condition.filter())
    }

    /// Takes the samples `condition` is true for (`take_w_condition`).
    /// Fails with [`Error::PreconditionNotMet`] for a condition of
    /// another reader.
    pub fn take_w_condition(&self, condition: &ReadCondition) -> Result<Vec<Sample<T>>> {
        Ok(self.select(self.check_condition(condition)?, None, true))
    }

    /// Reads the samples `condition` is true for (`read_w_condition`).
    pub fn read_w_condition(&self, condition: &ReadCondition) -> Result<Vec<Sample<T>>> {
        Ok(self.select(self.check_condition(condition)?, None, false))
    }

    /// Reads a status, clearing its changed flag.
    fn status<R: Default>(&self, k: StatusMask, f: impl FnOnce(&mut ReaderState) -> R) -> R {
        let mut g = self.shared.lock_any();
        g.readers
            .get_mut(&self.guid)
            .map(|r| {
                r.changes = r.changes & !k;
                f(r)
            })
            .unwrap_or_default()
    }

    /// The SUBSCRIPTION_MATCHED status: the writers matched. Reading it
    /// resets the `*_change` counts.
    pub fn subscription_matched_status(&self) -> MatchedStatus {
        self.status(StatusMask::SUBSCRIPTION_MATCHED, |r| {
            r.matched_status.take()
        })
    }

    /// The SAMPLE_LOST status: samples of matched writers that never
    /// arrived (gaps in their sequence numbers, for a BEST_EFFORT reader).
    /// Reading it resets `total_count_change`.
    pub fn sample_lost_status(&self) -> SampleLostStatus {
        self.status(StatusMask::SAMPLE_LOST, |r| r.samples.lost.take())
    }

    /// The writers matched now (`get_matched_publications`).
    pub fn matched_publications(&self) -> Vec<Guid> {
        let g = self.shared.lock_any();
        g.readers
            .get(&self.guid)
            .map(|r| r.matched.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The REQUESTED_INCOMPATIBLE_QOS status: writers of the same topic,
    /// type and partition that this reader did not match because of
    /// their QoS. Reading it resets `total_count_change`.
    pub fn requested_incompatible_qos_status(&self) -> IncompatibleQosStatus {
        self.status(StatusMask::REQUESTED_INCOMPATIBLE_QOS, |r| {
            r.incompatible.take()
        })
    }

    /// The SAMPLE_REJECTED status: samples dropped because of the
    /// reader's RESOURCE_LIMITS. Reading it resets `total_count_change`.
    pub fn sample_rejected_status(&self) -> SampleRejectedStatus {
        self.status(StatusMask::SAMPLE_REJECTED, |r| {
            r.samples.take_rejected_status()
        })
    }

    /// The LIVELINESS_CHANGED status: how many matched writers are alive
    /// and how many are not. Reading it resets the `*_change` counts.
    pub fn liveliness_changed_status(&self) -> LivelinessChangedStatus {
        self.status(StatusMask::LIVELINESS_CHANGED, |r| {
            r.liveliness_changed.take()
        })
    }

    /// The REQUESTED_DEADLINE_MISSED status: DEADLINE periods in which an
    /// alive instance got no sample. Reading it resets
    /// `total_count_change`.
    pub fn requested_deadline_missed_status(&self) -> DeadlineMissedStatus {
        self.status(StatusMask::REQUESTED_DEADLINE_MISSED, |r| {
            r.deadlines.missed.take()
        })
    }

    /// Waits until at least `count` writers are matched, or `timeout`
    /// passes. Returns whether they are.
    pub fn wait_for_publications(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.matched_publications().len() >= count {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl<T> Drop for DataReader<T> {
    fn drop(&mut self) {
        if self.owned {
            self.shared.remove_reader(&self.guid);
        }
    }
}

/// The listener of a reader, as kept by the participant: `listener`
/// called with a handle to the reader.
fn reader_listener<T: 'static>(
    samples: Arc<dyn SampleType<T>>,
    listener: impl DataReaderListener<T> + 'static,
) -> ReaderListener {
    Arc::new(move |shared: &Arc<Shared>, guid, event| {
        let reader = DataReader::<T> {
            shared: shared.clone(),
            guid,
            owned: false,
            samples: samples.clone(),
        };
        match event {
            ReaderEvent::DataAvailable => listener.on_data_available(&reader),
            ReaderEvent::SubscriptionMatched(s) => listener.on_subscription_matched(&reader, s),
            ReaderEvent::LivelinessChanged(s) => listener.on_liveliness_changed(&reader, s),
            ReaderEvent::RequestedDeadlineMissed(s) => {
                listener.on_requested_deadline_missed(&reader, s)
            }
            ReaderEvent::RequestedIncompatibleQos(s) => {
                listener.on_requested_incompatible_qos(&reader, s)
            }
            ReaderEvent::SampleRejected(s) => listener.on_sample_rejected(&reader, s),
            ReaderEvent::SampleLost(s) => listener.on_sample_lost(&reader, s),
        }
    })
}
