//! `Publisher` and `DataWriter`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::qos::{Reliability, ReliabilityKind};
use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, entity_kind};
use rustorb_typesupport::{ByteOrder, Encoding, TopicType};

use crate::error::Result;
use crate::participant::Shared;
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

    /// Creates a writer for `topic`.
    ///
    /// For now writers are BEST_EFFORT and VOLATILE: samples are sent once
    /// to the readers matched at the time of the write. Reliable writers
    /// come in a later stage (rustorb/rustorb#111, D2).
    pub fn create_datawriter<T: TopicType>(&self, topic: &Topic<T>) -> Result<DataWriter<T>> {
        let kind = if T::KEYED {
            entity_kind::USER_WRITER_WITH_KEY
        } else {
            entity_kind::USER_WRITER_NO_KEY
        };
        let guid = self.shared.new_guid(kind)?;
        let mut data = EndpointData::writer(guid, topic.name(), T::TYPE_NAME);
        data.reliability = Reliability {
            kind: ReliabilityKind::BestEffort,
            ..Reliability::writer_default()
        };
        self.shared.add_writer(data)?;
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
    pub fn write(&self, sample: &T) -> Result<()> {
        let payload = sample.to_payload(Encoding::Xcdr1, ByteOrder::NATIVE)?;
        let key_hash = if T::KEYED {
            Some(sample.key_hash()?)
        } else {
            None
        };
        self.shared.write(self.guid, payload, key_hash)
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
