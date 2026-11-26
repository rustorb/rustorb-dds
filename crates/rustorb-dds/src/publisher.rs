//! `Publisher` and `DataWriter`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, SampleIdentity, Time, entity_kind};
use rustorb_typesupport::{ByteOrder, Encoding};

use crate::condition::StatusCondition;
use crate::error::{Error, Result};
use crate::instance::InstanceHandle;
use crate::listener::{DataWriterListener, PublisherEntity, PublisherListener};
use crate::loan::{LoanCheck, PlainType, WriteInner, WriteLoan};
use crate::participant::{Shared, WriteOp, WriterEvent, WriterListener, WriterState};
use crate::qos::{
    DataWriterQos, DurabilityKind, PublisherQos, data_representation, duration_to_rtps,
};
use crate::status::{
    DeadlineMissedStatus, IncompatibleQosStatus, LivelinessLostStatus, MatchedStatus, StatusMask,
};
use crate::topic::Topic;
use crate::typed::SampleType;

/// A DDS publisher (DDS 1.4, 2.2.2.4.1): creates data writers.
#[derive(Clone)]
pub struct Publisher {
    shared: Arc<Shared>,
    entity: Arc<PublisherEntity>,
}

impl std::fmt::Debug for Publisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Publisher").finish_non_exhaustive()
    }
}

impl Publisher {
    pub(crate) fn new(shared: Arc<Shared>, entity: Arc<PublisherEntity>) -> Self {
        Publisher { shared, entity }
    }

    /// The publisher's QoS.
    pub fn qos(&self) -> &PublisherQos {
        &self.entity.qos
    }

    /// Sets the publisher's listener (`set_listener`), replacing any
    /// earlier one: it is called for the statuses in `mask` of its writers
    /// that their own listeners do not take (DDS 1.4, 2.2.4.3.1). See
    /// [`PublisherListener`].
    pub fn set_listener(&self, listener: impl PublisherListener + 'static, mask: StatusMask) {
        *self
            .entity
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((Arc::new(listener), mask));
    }

    /// Removes the publisher's listener.
    pub fn clear_listener(&self) {
        *self
            .entity
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Creates a writer for `topic` with the default QoS: RELIABLE,
    /// KEEP_LAST(1), VOLATILE.
    pub fn create_datawriter<T>(&self, topic: &Topic<T>) -> Result<DataWriter<T>> {
        self.create_datawriter_with_qos(topic, DataWriterQos::default())
    }

    /// Creates a writer for `topic`.
    ///
    /// A VOLATILE writer sends a reader the samples written after it was
    /// matched; a TRANSIENT_LOCAL one also those it kept from before, to
    /// TRANSIENT_LOCAL readers. A TRANSIENT or PERSISTENT one also, and
    /// its samples outlive it (see [`DataWriterQos::durability`]); a
    /// PERSISTENT one fails with [`Error::Io`] when its file cannot be
    /// written. A RELIABLE writer keeps samples until its reliable
    /// readers acknowledge them, and sends them again when asked; it also
    /// serves BEST_EFFORT readers. A BEST_EFFORT writer sends each sample
    /// once and matches only BEST_EFFORT readers.
    pub fn create_datawriter_with_qos<T>(
        &self,
        topic: &Topic<T>,
        qos: DataWriterQos,
    ) -> Result<DataWriter<T>> {
        self.create(topic, qos, None, |_| {})
    }

    /// Creates a writer whose announcement `f` changes before it is
    /// matched (as with the topic aliases of DDS-RPC).
    pub(crate) fn create_datawriter_with<T>(
        &self,
        topic: &Topic<T>,
        qos: DataWriterQos,
        f: impl FnOnce(&mut EndpointData),
    ) -> Result<DataWriter<T>> {
        self.create(topic, qos, None, f)
    }

    /// Creates a writer for `topic` with a listener for the statuses in
    /// `mask`, set before the writer is matched, so that it is told of
    /// the readers already there (DDS 1.4, 2.2.2.4.1.5).
    pub fn create_datawriter_with_listener<T: 'static>(
        &self,
        topic: &Topic<T>,
        qos: DataWriterQos,
        listener: impl DataWriterListener<T> + 'static,
        mask: StatusMask,
    ) -> Result<DataWriter<T>> {
        let encoding = encoding_of(&qos)?;
        let l = writer_listener(encoding, topic.samples.clone(), listener);
        self.create(topic, qos, Some((l, mask)), |_| {})
    }

    fn create<T>(
        &self,
        topic: &Topic<T>,
        qos: DataWriterQos,
        listener: Option<(WriterListener, StatusMask)>,
        f: impl FnOnce(&mut EndpointData),
    ) -> Result<DataWriter<T>> {
        let keyed = topic.samples.keyed();
        let kind = if keyed {
            entity_kind::USER_WRITER_WITH_KEY
        } else {
            entity_kind::USER_WRITER_NO_KEY
        };
        let encoding = encoding_of(&qos)?;
        let guid = self.shared.new_guid(kind)?;
        let type_name = topic.type_name();
        let mut data = EndpointData::writer(guid, topic.name(), type_name);
        data.type_information = self.shared.type_information(type_name)?;
        data.data_representation = qos.data_representation.clone();
        data.reliability = qos.reliability.to_rtps();
        data.durability = qos.durability;
        if qos.durability >= DurabilityKind::Transient {
            data.durability_service = Some(qos.durability_service.to_rtps());
        }
        data.deadline = duration_to_rtps(qos.deadline);
        data.liveliness = qos.liveliness.to_rtps();
        data.user_data = qos.user_data.clone();
        data.data_tags = qos.data_tags.clone();
        data.partitions = self.entity.qos.partition.clone();
        f(&mut data);
        self.shared.add_writer(
            data,
            qos,
            topic.samples.key_ops(),
            listener,
            self.entity.clone(),
        )?;
        Ok(DataWriter {
            shared: self.shared.clone(),
            guid,
            encoding,
            owned: true,
            samples: topic.samples.clone(),
            loan: Arc::default(),
        })
    }
}

/// A DDS data writer (DDS 1.4, 2.2.2.4.2) of samples of type `T`: a
/// [`TopicType`](crate::TopicType), or [`DynamicData`](crate::DynamicData)
/// for a topic of a type known at run time (a `DynamicDataWriter`,
/// DDS-XTypes 1.3, 7.5.2.12).
///
/// Dropping it tells the other participants it is gone.
pub struct DataWriter<T> {
    pub(crate) shared: Arc<Shared>,
    pub(crate) guid: Guid,
    /// How samples are serialized: the writer's data representation.
    encoding: Encoding,
    /// Whether dropping this handle deletes the writer: not for the
    /// handle given to a listener.
    owned: bool,
    samples: Arc<dyn SampleType<T>>,
    /// What loaned samples showed of the type's layout.
    #[cfg_attr(not(feature = "shm"), allow(dead_code))]
    loan: Arc<LoanCheck>,
}

impl<T> std::fmt::Debug for DataWriter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataWriter")
            .field("guid", &self.guid)
            .field("type_name", &self.samples.type_name())
            .finish()
    }
}

impl<T> DataWriter<T> {
    /// The writer's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    /// Publishes a sample: serializes it in XCDR1, or XCDR2 if that is
    /// the writer's data representation (in the host's byte order), and
    /// sends it to every matched reader.
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
        self.write_w_params(
            sample,
            &WriteParams {
                source_timestamp: Some(timestamp),
                ..WriteParams::default()
            },
        )
        .map(|_| ())
    }

    /// Like [`Self::write`], with the parameters given; returns the
    /// identity of the sample written (its writer and sequence number),
    /// which a reply names as its related sample (DDS-RPC 1.0, 7.8.2).
    pub fn write_w_params(&self, sample: &T, params: &WriteParams) -> Result<SampleIdentity> {
        self.write_mapped(sample, params, |p| p)
    }

    /// Like [`Self::write_w_params`], with the serialized payload passed
    /// through `map` before it is written.
    pub(crate) fn write_mapped(
        &self,
        sample: &T,
        params: &WriteParams,
        map: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> Result<SampleIdentity> {
        let payload = self
            .samples
            .to_payload(sample, self.encoding, ByteOrder::NATIVE)?;
        let payload = map(payload);
        let sequence_number = self.shared.write(
            self.guid,
            self.samples.instance_of(sample)?,
            WriteOp::Value(
                payload.into(),
                params.related_sample_identity,
                params.directed_to,
            ),
            params.source_timestamp.unwrap_or_else(Time::now),
        )?;
        Ok(SampleIdentity {
            writer_guid: self.guid,
            sequence_number,
        })
    }

    /// Registers the instance of `instance` (whose key members are what
    /// counts) and returns its handle (`register_instance`). This sends
    /// nothing: readers learn of the instance when it is written.
    pub fn register_instance(&self, instance: &T) -> Result<InstanceHandle> {
        let h = self.samples.instance_of(instance)?;
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
        self.shared
            .write(
                self.guid,
                self.samples.instance_of(instance)?,
                WriteOp::Lifecycle {
                    dispose,
                    unregister: true,
                },
                timestamp,
            )
            .map(|_| ())
    }

    /// Disposes an instance (`dispose`): readers see it
    /// NOT_ALIVE_DISPOSED until it is written again. The instance stays
    /// registered (or is registered).
    pub fn dispose(&self, instance: &T) -> Result<()> {
        self.dispose_w_timestamp(instance, Time::now())
    }

    /// Like [`Self::dispose`], with the source timestamp given.
    pub fn dispose_w_timestamp(&self, instance: &T, timestamp: Time) -> Result<()> {
        self.shared
            .write(
                self.guid,
                self.samples.instance_of(instance)?,
                WriteOp::Lifecycle {
                    dispose: true,
                    unregister: false,
                },
                timestamp,
            )
            .map(|_| ())
    }

    /// The handle of the instance of `instance` if it is registered
    /// (`lookup_instance`).
    pub fn lookup_instance(&self, instance: &T) -> Option<InstanceHandle> {
        let h = self.samples.instance_of(instance).ok()?;
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
        self.status(StatusMask::OFFERED_INCOMPATIBLE_QOS, |w| {
            w.incompatible.take()
        })
    }

    /// Reads a status, clearing its changed flag.
    fn status<R: Default>(&self, k: StatusMask, f: impl FnOnce(&mut WriterState) -> R) -> R {
        let mut g = self.shared.lock_any();
        g.writers
            .get_mut(&self.guid)
            .map(|w| {
                w.changes = w.changes & !k;
                f(w)
            })
            .unwrap_or_default()
    }

    /// The PUBLICATION_MATCHED status: the readers matched. Reading it
    /// resets the `*_change` counts.
    pub fn publication_matched_status(&self) -> MatchedStatus {
        self.status(StatusMask::PUBLICATION_MATCHED, |w| w.matched_status.take())
    }

    /// Sets the listener (`set_listener`), replacing any earlier one:
    /// its methods are called for the statuses in `mask` when they
    /// change. See [`DataWriterListener`].
    pub fn set_listener(&self, listener: impl DataWriterListener<T> + 'static, mask: StatusMask)
    where
        T: 'static,
    {
        let l = writer_listener(self.encoding, self.samples.clone(), listener);
        // The one before is dropped with no lock held (#117).
        let before = {
            let mut g = self.shared.lock_any();
            g.writers
                .get_mut(&self.guid)
                .and_then(|w| w.listener.replace((l, mask)))
        };
        drop(before);
    }

    /// Removes the listener.
    pub fn clear_listener(&self) {
        let before = {
            let mut g = self.shared.lock_any();
            g.writers
                .get_mut(&self.guid)
                .and_then(|w| w.listener.take())
        };
        drop(before);
    }

    /// The statuses changed since they were last read
    /// (`get_status_changes`).
    pub fn status_changes(&self) -> StatusMask {
        let g = self.shared.lock_any();
        g.writers
            .get(&self.guid)
            .map_or(StatusMask::NONE, |w| w.changes)
    }

    /// The writer's StatusCondition (`get_statuscondition`).
    pub fn status_condition(&self) -> StatusCondition {
        StatusCondition::new(self.shared.clone(), self.guid)
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
        self.status(StatusMask::LIVELINESS_LOST, |w| w.liveliness_lost.take())
    }

    /// The OFFERED_DEADLINE_MISSED status: DEADLINE periods in which a
    /// registered instance was not written. Reading it resets
    /// `total_count_change`.
    pub fn offered_deadline_missed_status(&self) -> DeadlineMissedStatus {
        self.status(StatusMask::OFFERED_DEADLINE_MISSED, |w| {
            w.deadlines.missed.take()
        })
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

impl<T: PlainType> DataWriter<T> {
    /// Lends a sample to fill and then publish with
    /// [`Self::write_loaned`] (zero-copy): a sample in the writer's pool
    /// in shared memory, which the readers of this host read in place,
    /// with no serialization and no copy. It holds zeros, or an earlier
    /// sample of the writer when its slot is used again: set every member.
    ///
    /// The sample goes by reference when every reader matched is of this
    /// participant or reached by shared memory, the writer is VOLATILE
    /// and DDS Security is off; otherwise its octets are sent, as for
    /// [`Self::write`], still without serialization. The sample is on
    /// the heap, and serialized when written, when the pool cannot be
    /// used: without shared memory (feature `shm` and
    /// [`ParticipantConfig::shared_memory`](crate::ParticipantConfig)),
    /// with DDS Security, when every slot of the pool is held, or when
    /// the layout of `T` is not its serialized form (see [`PlainType`]).
    ///
    /// A slot of the pool is used again once the writer's history no
    /// longer has its sample (and it is not one of the newest few) and
    /// every reader has dropped it.
    pub fn loan_sample(&self) -> Result<WriteLoan<T>> {
        #[cfg(feature = "shm")]
        if let Some(header) = self.plain_header()
            && std::mem::align_of::<T>() <= 8
            && let Some(pool) = self
                .shared
                .writer_pool(&self.guid, 4 + std::mem::size_of::<T>())?
            && let Some(mut slot) = pool.acquire()
        {
            slot.bytes_mut()[..4].copy_from_slice(&header);
            return Ok(WriteLoan {
                inner: WriteInner::Shared(slot, std::marker::PhantomData),
            });
        }
        Ok(WriteLoan::heap())
    }

    /// Publishes a loaned sample, as [`Self::write`] does a sample.
    pub fn write_loaned(&self, sample: WriteLoan<T>) -> Result<()> {
        self.write_loaned_w_timestamp(sample, Time::now())
    }

    /// Like [`Self::write_loaned`], with the source timestamp given.
    pub fn write_loaned_w_timestamp(&self, sample: WriteLoan<T>, timestamp: Time) -> Result<()> {
        let h = self.samples.instance_of(&sample)?;
        let payload = match sample.inner {
            #[cfg(feature = "shm")]
            WriteInner::Shared(slot, _) => {
                let len = 4 + std::mem::size_of::<T>();
                if self.loan.verified() {
                    crate::instance::Payload::Loaned(slot.publish(len))
                } else {
                    // Until a sample shows that the layout is the
                    // serialized form, the samples are serialized.
                    let serialized = self.serialize(WriteLoan::<T>::value_of(&slot))?;
                    if serialized == slot.bytes()[..len] {
                        self.loan.set_verified();
                        crate::instance::Payload::Loaned(slot.publish(len))
                    } else {
                        serialized.into()
                    }
                }
            }
            WriteInner::Heap(b) => self.serialize(&b)?.into(),
        };
        self.shared
            .write(self.guid, h, WriteOp::Value(payload, None, None), timestamp)
            .map(|_| ())
    }

    fn serialize(&self, v: &T) -> Result<Vec<u8>> {
        self.samples.to_payload(v, self.encoding, ByteOrder::NATIVE)
    }

    /// The encapsulation of a sample whose memory is its serialized form:
    /// `None` if the serialized form of a zero `T` is not as long as one.
    #[cfg(feature = "shm")]
    fn plain_header(&self) -> Option<[u8; 4]> {
        *self.loan.header.get_or_init(|| {
            let p = self.serialize(&crate::loan::zeroed_box::<T>()).ok()?;
            (p.len() == 4 + std::mem::size_of::<T>()).then(|| [p[0], p[1], p[2], p[3]])
        })
    }
}

impl<T> Drop for DataWriter<T> {
    fn drop(&mut self) {
        if self.owned {
            self.shared.remove_writer(&self.guid);
        }
    }
}

/// The listener of a writer, as kept by the participant: `listener`
/// called with a handle to the writer.
fn writer_listener<T: 'static>(
    encoding: Encoding,
    samples: Arc<dyn SampleType<T>>,
    listener: impl DataWriterListener<T> + 'static,
) -> WriterListener {
    Arc::new(move |shared: &Arc<Shared>, guid, event| {
        let writer = DataWriter::<T> {
            shared: shared.clone(),
            guid,
            encoding,
            owned: false,
            samples: samples.clone(),
            loan: Arc::default(),
        };
        match event {
            WriterEvent::PublicationMatched(s) => listener.on_publication_matched(&writer, s),
            WriterEvent::LivelinessLost(s) => listener.on_liveliness_lost(&writer, s),
            WriterEvent::OfferedDeadlineMissed(s) => {
                listener.on_offered_deadline_missed(&writer, s)
            }
            WriterEvent::OfferedIncompatibleQos(s) => {
                listener.on_offered_incompatible_qos(&writer, s)
            }
        }
    })
}

/// How a writer with `qos` serializes samples: its first data
/// representation.
fn encoding_of(qos: &DataWriterQos) -> Result<Encoding> {
    match qos.data_representation.first() {
        None | Some(&data_representation::XCDR) => Ok(Encoding::Xcdr1),
        Some(&data_representation::XCDR2) => Ok(Encoding::Xcdr2),
        Some(_) => Err(Error::Unsupported(
            "a data representation other than XCDR and XCDR2",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xcdr2_identifiers_of_table_60() {
        // rustorb-cdr writes the identifiers of DDS-XTypes Table 60
        // (`CDR2_LE` is `0x0007`), which Cyclone DDS 0.10.4 takes; it
        // drops those of Table 39 (`0x0011`).
        let samples = crate::typed::Typed::<crate::test_shape::Shape>::new();
        for (order, id) in [(ByteOrder::Little, 0x07), (ByteOrder::Big, 0x06)] {
            let p = samples
                .to_payload(&crate::test_shape::Shape::default(), Encoding::Xcdr2, order)
                .unwrap();
            assert_eq!(p[..2], [0x00, id]);
        }
    }
}

/// The parameters of [`DataWriter::write_w_params`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteParams {
    /// The source timestamp; `None` for now.
    pub source_timestamp: Option<Time>,
    /// The sample this one answers, sent with it in-line
    /// (`PID_RELATED_SAMPLE_IDENTITY`, DDS-RPC 1.0, 7.8.2), which the
    /// readers find in [`SampleInfo::related_sample_identity`](crate::SampleInfo::related_sample_identity).
    pub related_sample_identity: Option<SampleIdentity>,
    /// The one matched reader to send the sample to; the others are told
    /// it is not for them (a GAP, as for a sample a writer filters out,
    /// DDSI-RTPS 2.5, 8.7.3.4). A [`Replier`](crate::rpc::Replier) sends
    /// a reply so to the requester that asked. `None` sends it to all.
    pub directed_to: Option<Guid>,
}
