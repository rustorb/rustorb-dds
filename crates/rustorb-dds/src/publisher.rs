//! `Publisher` and `DataWriter`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, entity_kind};
use rustorb_typesupport::{ByteOrder, Encoding, TopicType};

use crate::error::Result;
use crate::participant::Shared;
use crate::qos::DataWriterQos;
use crate::topic::Topic;

/// A DDS publisher (DDS 1.4, 2.2.2.4.1): creates data writers.
pub struct Publisher {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publisher").finish_non_exhaustive()
    }
}

impl Publisher {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Publisher { shared }
    }

    /// Creates a writer for `topic` with the default QoS: RELIABLE,
    /// KEEP_LAST(1), VOLATILE.
    pub fn create_datawriter<T: TopicType>(&self, topic: &Topic<T>) -> Result<DataWriter<T>> {
        self.create_datawriter_with_qos(topic, DataWriterQos::default())
    }

    /// Creates a writer for `topic`.
    ///
    /// Writers are VOLATILE: a reader gets the samples written after it
    /// was matched. A RELIABLE writer keeps samples until its reliable
    /// readers acknowledge them, and sends them again when asked; it also
    /// serves BEST_EFFORT readers. A BEST_EFFORT writer sends each sample
    /// once and matches only BEST_EFFORT readers.
    pub fn create_datawriter_with_qos<T: TopicType>(
        &self,
        topic: &Topic<T>,
        qos: DataWriterQos,
    ) -> Result<DataWriter<T>> {
        let kind = if T::KEYED {
            entity_kind::USER_WRITER_WITH_KEY
        } else {
            entity_kind::USER_WRITER_NO_KEY
        };
        let guid = self.shared.new_guid(kind)?;
        let mut data = EndpointData::writer(guid, topic.name(), T::TYPE_NAME);
        data.reliability = qos.reliability.to_rtps();
        self.shared.add_writer(data, qos)?;
        Ok(DataWriter {
            shared: self.shared.clone(),
            guid,
            _type: PhantomData,
        })
    }
}

/// A DDS data writer (DDS 1.4, 2.2.2.4.2) of samples of type `T`.
///
/// Dropping it tells the other participants it is gone.
pub struct DataWriter<T: TopicType> {
    shared: Arc<Shared>,
    guid: Guid,
    _type: PhantomData<fn(T)>,
}

impl<T: TopicType> std::fmt::Debug for DataWriter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataWriter")
            .field("guid", &self.guid)
            .field("type_name", &T::TYPE_NAME)
            .finish()
    }
}

impl<T: TopicType> DataWriter<T> {
    /// The writer's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    /// Publishes a sample: serializes it in XCDR1 (in the host's byte
    /// order) and sends it to every matched reader.
    ///
    /// A RELIABLE writer with KEEP_ALL history and `max_samples` set
    /// waits, when its history is full, for its readers to acknowledge
    /// samples; after `max_blocking_time` it fails with
    /// [`Error::Timeout`](crate::Error::Timeout).
    pub fn write(&self, sample: &T) -> Result<()> {
        let payload = sample.to_payload(Encoding::Xcdr1, ByteOrder::NATIVE)?;
        let key_hash = if T::KEYED {
            Some(sample.key_hash()?)
        } else {
            None
        };
        self.shared.write(self.guid, payload, key_hash)
    }

    /// Waits until every matched RELIABLE reader has acknowledged every
    /// sample written (`wait_for_acknowledgments`). Fails with
    /// [`Error::Timeout`](crate::Error::Timeout) if `max_wait` passes
    /// first.
    pub fn wait_for_acknowledgments(&self, max_wait: Duration) -> Result<()> {
        self.shared.wait_for_acknowledgments(&self.guid, max_wait)
    }

    /// The readers matched now (`get_matched_subscriptions`).
    pub fn matched_subscriptions(&self) -> Vec<Guid> {
        let g = self.shared.lock_any();
        g.writers
            .get(&self.guid)
            .map(|w| w.matched.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Waits until at least `count` readers are matched, or `timeout`
    /// passes. Returns whether they are.
    pub fn wait_for_subscriptions(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.matched_subscriptions().len() >= count {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl<T: TopicType> Drop for DataWriter<T> {
    fn drop(&mut self) {
        self.shared.remove_writer(&self.guid);
    }
}
