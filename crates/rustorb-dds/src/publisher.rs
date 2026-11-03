//! `Publisher` and `DataWriter`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, Time, entity_kind};
use rustorb_typesupport::{ByteOrder, Encoding, TopicType};

use crate::error::{Error, Result};
use crate::instance::InstanceHandle;
use crate::participant::{Shared, WriteOp};
use crate::qos::{DataWriterQos, DurabilityKind, PublisherQos, duration_to_rtps};
use crate::status::{DeadlineMissedStatus, IncompatibleQosStatus, LivelinessLostStatus};
use crate::topic::Topic;
use crate::typed::instance_of;

/// A DDS publisher (DDS 1.4, 2.2.2.4.1): creates data writers.
pub struct Publisher {
    shared: Arc<Shared>,
    qos: PublisherQos,
}

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publisher").finish_non_exhaustive()
    }
}

impl Publisher {
    pub(crate) fn new(shared: Arc<Shared>, qos: PublisherQos) -> Self {
        Publisher { shared, qos }
    }

    /// The publisher's QoS.
    pub fn qos(&self) -> &PublisherQos {
        &self.qos
    }

    /// Creates a writer for `topic` with the default QoS: RELIABLE,
    /// KEEP_LAST(1), VOLATILE.
    pub fn create_datawriter<T: TopicType>(&self, topic: &Topic<T>) -> Result<DataWriter<T>> {
        self.create_datawriter_with_qos(topic, DataWriterQos::default())
    }

    /// Creates a writer for `topic`.
    ///
    /// A VOLATILE writer sends a reader the samples written after it was
    /// matched; a TRANSIENT_LOCAL one also those it kept from before, to
    /// TRANSIENT_LOCAL readers. TRANSIENT and PERSISTENT fail with
    /// [`Error::Unsupported`]. A RELIABLE writer keeps samples until its reliable
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
        if qos.durability > DurabilityKind::TransientLocal {
            return Err(Error::Unsupported("TRANSIENT and PERSISTENT durability"));
        }
        let guid = self.shared.new_guid(kind)?;
        let mut data = EndpointData::writer(guid, topic.name(), T::TYPE_NAME);
        data.reliability = qos.reliability.to_rtps();
        data.durability = qos.durability;
        data.deadline = duration_to_rtps(qos.deadline);
        data.liveliness = qos.liveliness.to_rtps();
        data.partitions = self.qos.partition.clone();
        self.shared.add_writer(data, qos, T::KEYED)?;
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
    ///
    /// Writing registers the sample's instance if it is not yet. KEEP_LAST
    /// keeps the newest samples of each instance.
    pub fn write(&self, sample: &T) -> Result<()> {
        self.write_w_timestamp(sample, Time::now())
    }

    /// Like [`Self::write`], with the source timestamp given.
    pub fn write_w_timestamp(&self, sample: &T, timestamp: Time) -> Result<()> {
        let payload = sample.to_payload(Encoding::Xcdr1, ByteOrder::NATIVE)?;
        self.shared.write(
            self.guid,
            instance_of(sample)?,
            WriteOp::Value(payload),
            timestamp,
        )
    }

    /// Registers the instance of `instance` (whose key members are what
    /// counts) and returns its handle (`register_instance`). This sends
    /// nothing: readers learn of the instance when it is written.
    pub fn register_instance(&self, instance: &T) -> Result<InstanceHandle> {
        let h = instance_of(instance)?;
        self.shared.register_instance(&self.guid, h)?;
        Ok(h)
    }

    /// Unregisters an instance (`unregister_instance`): this writer has
    /// nothing more to say about it. A reader's instance becomes
    /// NOT_ALIVE_NO_WRITERS once no writer has it registered, or
    /// NOT_ALIVE_DISPOSED when `autodispose_unregistered_instances` is
    /// set. Fails with
    /// [`Error::PreconditionNotMet`](crate::Error::PreconditionNotMet)
    /// when the instance is not registered.
    pub fn unregister_instance(&self, instance: &T) -> Result<()> {
        self.unregister_instance_w_timestamp(instance, Time::now())
    }

    /// Like [`Self::unregister_instance`], with the source timestamp
    /// given.
    pub fn unregister_instance_w_timestamp(&self, instance: &T, timestamp: Time) -> Result<()> {
        let dispose = self.shared.autodispose_of(&self.guid);
        self.shared.write(
            self.guid,
            instance_of(instance)?,
            WriteOp::Lifecycle {
                dispose,
                unregister: true,
            },
            timestamp,
        )
    }

    /// Disposes an instance (`dispose`): readers see it
    /// NOT_ALIVE_DISPOSED until it is written again. The instance stays
    /// registered (or is registered).
    pub fn dispose(&self, instance: &T) -> Result<()> {
        self.dispose_w_timestamp(instance, Time::now())
    }

    /// Like [`Self::dispose`], with the source timestamp given.
    pub fn dispose_w_timestamp(&self, instance: &T, timestamp: Time) -> Result<()> {
        self.shared.write(
            self.guid,
            instance_of(instance)?,
            WriteOp::Lifecycle {
                dispose: true,
                unregister: false,
            },
            timestamp,
        )
    }

    /// The handle of the instance of `instance` if it is registered
    /// (`lookup_instance`).
    pub fn lookup_instance(&self, instance: &T) -> Option<InstanceHandle> {
        let h = instance_of(instance).ok()?;
        let g = self.shared.lock_any();
        g.writers
            .get(&self.guid)
            .filter(|w| w.instances.contains_key(&h))
            .map(|_| h)
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

    /// The OFFERED_INCOMPATIBLE_QOS status: readers of the same topic,
    /// type and partition that this writer did not match because of its
    /// QoS. Reading it resets `total_count_change`.
    pub fn offered_incompatible_qos_status(&self) -> IncompatibleQosStatus {
        let mut g = self.shared.lock_any();
        g.writers
            .get_mut(&self.guid)
            .map(|w| w.incompatible.take())
            .unwrap_or_default()
    }

    /// Asserts the writer's liveliness (`assert_liveliness`, DDS 1.4,
    /// 2.2.2.4.2.22), for a MANUAL_BY_PARTICIPANT or MANUAL_BY_TOPIC
    /// writer that has nothing to write; writing asserts it too. It does
    /// nothing for an AUTOMATIC writer, whose participant asserts it.
    pub fn assert_liveliness(&self) -> Result<()> {
        self.shared.assert_writer_liveliness(&self.guid)
    }

    /// The LIVELINESS_LOST status: how many times the writer's lease
    /// passed without it asserting its liveliness. Reading it resets
    /// `total_count_change`.
    pub fn liveliness_lost_status(&self) -> LivelinessLostStatus {
        let mut g = self.shared.lock_any();
        g.writers
            .get_mut(&self.guid)
            .map(|w| w.liveliness_lost.take())
            .unwrap_or_default()
    }

    /// The OFFERED_DEADLINE_MISSED status: DEADLINE periods in which a
    /// registered instance was not written. Reading it resets
    /// `total_count_change`.
    pub fn offered_deadline_missed_status(&self) -> DeadlineMissedStatus {
        let mut g = self.shared.lock_any();
        g.writers
            .get_mut(&self.guid)
            .map(|w| w.deadlines.missed.take())
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
