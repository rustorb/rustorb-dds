//! `Subscriber` and `DataReader`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::{Guid, SequenceNumber, Time, entity_kind};
use rustorb_typesupport::TopicType;

use crate::error::Result;
use crate::participant::{RawSample, Shared};
use crate::qos::DataReaderQos;
use crate::topic::Topic;

/// A DDS subscriber (DDS 1.4, 2.2.2.5.1): creates data readers.
pub struct Subscriber {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriber").finish_non_exhaustive()
    }
}

impl Subscriber {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Subscriber { shared }
    }

    /// Creates a reader for `topic` with the default QoS: BEST_EFFORT,
    /// KEEP_LAST(1), VOLATILE.
    pub fn create_datareader<T: TopicType>(&self, topic: &Topic<T>) -> Result<DataReader<T>> {
        self.create_datareader_with_qos(topic, DataReaderQos::default())
    }

    /// Creates a reader for `topic`.
    ///
    /// A RELIABLE reader gets the samples of each writer in order and
    /// without gaps, asking for those lost on the way; it matches only
    /// RELIABLE writers. A BEST_EFFORT reader takes what arrives, in
    /// order, and matches any writer.
    pub fn create_datareader_with_qos<T: TopicType>(
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
        self.shared.add_reader(data, qos)?;
        Ok(DataReader {
            shared: self.shared.clone(),
            guid,
            _type: PhantomData,
        })
    }
}

/// Information about a received sample (a part of DDS 1.4 `SampleInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleInfo {
    /// The writer that sent it (`publication_handle`).
    pub writer: Guid,
    /// Its sequence number from that writer.
    pub sequence_number: SequenceNumber,
    /// When the writer wrote it, if the writer said
    /// (`source_timestamp`).
    pub source_timestamp: Option<Time>,
    /// Whether it was returned by [`DataReader::read`] before
    /// (`sample_state` READ).
    pub read: bool,
}

/// A sample with its information.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample<T> {
    /// The value.
    pub data: T,
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

fn decode<T: TopicType>(s: &RawSample) -> Option<Sample<T>> {
    let data = T::from_payload(&s.payload).ok()?;
    Some(Sample {
        data,
        info: SampleInfo {
            writer: s.writer,
            sequence_number: s.sequence_number,
            source_timestamp: s.source_timestamp,
            read: s.read,
        },
    })
}

impl<T: TopicType> DataReader<T> {
    /// The reader's GUID.
    pub fn guid(&self) -> Guid {
        self.guid
    }

    /// Removes and returns the samples received, oldest first.
    pub fn take(&self) -> Vec<Sample<T>> {
        let raw: Vec<RawSample> = {
            let mut g = self.shared.lock_any();
            match g.readers.get_mut(&self.guid) {
                Some(r) => r.queue.drain(..).collect(),
                None => Vec::new(),
            }
        };
        raw.iter().filter_map(decode).collect()
    }

    /// Returns the samples received, oldest first, and leaves them in the
    /// reader, marked as read.
    pub fn read(&self) -> Vec<Sample<T>> {
        let mut g = self.shared.lock_any();
        let Some(r) = g.readers.get_mut(&self.guid) else {
            return Vec::new();
        };
        let out = r.queue.iter().filter_map(decode).collect();
        for s in r.queue.iter_mut() {
            s.read = true;
        }
        out
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
                .is_some_and(|r| r.queue.iter().any(|s| !s.read));
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
