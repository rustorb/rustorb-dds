//! `Subscriber` and `DataReader`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, entity_kind};
use rustorb_typesupport::TopicType;

use crate::error::Result;
use crate::instance::{InstanceHandle, SampleInfo, Selected, StateFilter};
use crate::participant::Shared;
use crate::qos::{DataReaderQos, SubscriberQos, duration_to_rtps};
use crate::status::{
    DeadlineMissedStatus, IncompatibleQosStatus, LivelinessChangedStatus, SampleRejectedStatus,
};
use crate::topic::Topic;
use crate::typed::{Typed, instance_of};

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
    /// KEEP_LAST(1), VOLATILE.
    pub fn create_datareader<T: TopicType + 'static>(
        &self,
        topic: &Topic<T>,
    ) -> Result<DataReader<T>> {
        self.create_datareader_with_qos(topic, DataReaderQos::default())
    }

    /// Creates a reader for `topic`.
    ///
    /// A RELIABLE reader gets the samples of each writer in order and
    /// without gaps, asking for those lost on the way; it matches only
    /// RELIABLE writers. A BEST_EFFORT reader takes what arrives, in
    /// order, and matches any writer.
    pub fn create_datareader_with_qos<T: TopicType + 'static>(
        &self,
        topic: &Topic<T>,
        qos: DataReaderQos,
    ) -> Result<DataReader<T>> {
        let kind = if T::KEYED {
            entity_kind::USER_READER_WITH_KEY
        } else {
            entity_kind::USER_READER_NO_KEY
        };
        let guid = self.shared.new_guid(kind)?;
        let mut data = EndpointData::reader(guid, topic.name(), T::TYPE_NAME);
        data.reliability = qos.reliability.to_rtps();
        data.durability = qos.durability;
        data.deadline = duration_to_rtps(qos.deadline);
        data.liveliness = qos.liveliness.to_rtps();
        data.partitions = self.qos.partition.clone();
        self.shared
            .add_reader(data, qos, Arc::new(Typed::<T>::new()))?;
        Ok(DataReader {
            shared: self.shared.clone(),
            guid,
            _type: PhantomData,
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

/// A DDS data reader (DDS 1.4, 2.2.2.5.3) of samples of type `T`.
///
/// Samples that cannot be deserialized as `T` are dropped when they are
/// read or taken. Dropping the reader tells the other participants it is
/// gone.
pub struct DataReader<T: TopicType> {
    shared: Arc<Shared>,
    guid: Guid,
    _type: PhantomData<fn() -> T>,
}

impl<T: TopicType> std::fmt::Debug for DataReader<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataReader")
            .field("guid", &self.guid)
            .field("type_name", &T::TYPE_NAME)
            .finish()
    }
}

fn decode<T: TopicType>((payload, info): Selected) -> Option<Sample<T>> {
    let data = match payload {
        Some(p) => Some(T::from_payload(&p).ok()?),
        None => None,
    };
    Some(Sample { data, info })
}

impl<T: TopicType> DataReader<T> {
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
                Some(r) => r.samples.select(filter, instance, take),
                None => Vec::new(),
            }
        };
        raw.into_iter().filter_map(decode).collect()
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
            match r.samples.next_instance(previous, filter) {
                Some(h) => r.samples.select(filter, Some(h), take),
                None => Vec::new(),
            }
        };
        raw.into_iter().filter_map(decode).collect()
    }

    /// The handle of the instance of `instance` (`lookup_instance`), if
    /// the reader has it.
    pub fn lookup_instance(&self, instance: &T) -> Option<InstanceHandle> {
        let h = instance_of(instance).ok()?;
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
        T::from_payload(r.samples.last_value(&instance)?).ok()
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

    /// Sets the function called when a sample arrives (the
    /// `on_data_available` listener), replacing any earlier one.
    ///
    /// It runs on the participant's thread, or on the writer's thread for
    /// a writer in the same participant, and must not block for long. It
    /// may call [`DataReader::take`] through its own handle, or send to a
    /// channel:
    ///
    /// ```no_run
    /// # fn f(reader: &rustorb_dds::DataReader<impl rustorb_typesupport::TopicType>) {
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// reader.set_listener(move || {
    ///     let _ = tx.send(());
    /// });
    /// # }
    /// ```
    pub fn set_listener(&self, f: impl Fn() + Send + Sync + 'static) {
        let mut g = self.shared.lock_any();
        if let Some(r) = g.readers.get_mut(&self.guid) {
            r.listener = Some(Arc::new(f));
        }
    }

    /// Removes the listener.
    pub fn clear_listener(&self) {
        let mut g = self.shared.lock_any();
        if let Some(r) = g.readers.get_mut(&self.guid) {
            r.listener = None;
        }
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
        let mut g = self.shared.lock_any();
        g.readers
            .get_mut(&self.guid)
            .map(|r| r.incompatible.take())
            .unwrap_or_default()
    }

    /// The SAMPLE_REJECTED status: samples dropped because of the
    /// reader's RESOURCE_LIMITS. Reading it resets `total_count_change`.
    pub fn sample_rejected_status(&self) -> SampleRejectedStatus {
        let mut g = self.shared.lock_any();
        g.readers
            .get_mut(&self.guid)
            .map(|r| r.samples.take_rejected_status())
            .unwrap_or_default()
    }

    /// The LIVELINESS_CHANGED status: how many matched writers are alive
    /// and how many are not. Reading it resets the `*_change` counts.
    pub fn liveliness_changed_status(&self) -> LivelinessChangedStatus {
        let mut g = self.shared.lock_any();
        g.readers
            .get_mut(&self.guid)
            .map(|r| r.liveliness_changed.take())
            .unwrap_or_default()
    }

    /// The REQUESTED_DEADLINE_MISSED status: DEADLINE periods in which an
    /// alive instance got no sample. Reading it resets
    /// `total_count_change`.
    pub fn requested_deadline_missed_status(&self) -> DeadlineMissedStatus {
        let mut g = self.shared.lock_any();
        g.readers
            .get_mut(&self.guid)
            .map(|r| r.deadlines.missed.take())
            .unwrap_or_default()
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

impl<T: TopicType> Drop for DataReader<T> {
    fn drop(&mut self) {
        self.shared.remove_reader(&self.guid);
    }
}
