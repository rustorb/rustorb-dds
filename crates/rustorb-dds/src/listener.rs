//! Listeners (DDS 1.4, 2.2.4.3).
//!
//! A status change goes to the most specific listener that takes it
//! (2.2.4.3.1): that of the writer or reader, else that of its publisher
//! or subscriber, else that of the participant. INCONSISTENT_TOPIC goes to
//! the topic's listener, else the participant's. New data (2.2.4.3.2)
//! goes first to `on_data_on_readers` of the subscriber's listener (else
//! the participant's), and only when neither takes DATA_ON_READERS to
//! `on_data_available` along the same chain as the other statuses.

use std::sync::{Arc, Mutex};

use rustorb_rtps::Guid;

use crate::participant::Shared;
use crate::publisher::DataWriter;
use crate::qos::{PublisherQos, SubscriberQos};
use crate::status::{
    DeadlineMissedStatus, IncompatibleQosStatus, InconsistentTopicStatus, LivelinessChangedStatus,
    LivelinessLostStatus, MatchedStatus, SampleLostStatus, SampleRejectedStatus, StatusMask,
};
use crate::subscriber::{DataReader, Subscriber};

/// The listener of a [`DataReader`] (`DataReaderListener`, DDS 1.4,
/// 2.2.4.3.9): called when one of its statuses changes. Each method does
/// nothing unless implemented.
///
/// A method is called only for the statuses in the mask given with
/// [`DataReader::set_listener`]. The status is passed read: its
/// `*_change` counts are reset, and its StatusCondition does not see the
/// change. For the others, the change is kept for the status getters and
/// the StatusCondition.
///
/// The listener runs on the participant's thread, or on the writing
/// thread for a writer in the same participant, without any lock held:
/// it may read or take through the reader handle it is given, but should
/// not block for long.
#[allow(unused_variables)]
pub trait DataReaderListener<T>: Send + Sync {
    /// DATA_AVAILABLE: new samples, or new instance states, to read.
    fn on_data_available(&self, reader: &DataReader<T>) {}
    /// SUBSCRIPTION_MATCHED: a writer was matched or unmatched.
    fn on_subscription_matched(&self, reader: &DataReader<T>, status: MatchedStatus) {}
    /// LIVELINESS_CHANGED: a matched writer became alive or not alive.
    fn on_liveliness_changed(&self, reader: &DataReader<T>, status: LivelinessChangedStatus) {}
    /// REQUESTED_DEADLINE_MISSED: an instance got no sample within the
    /// DEADLINE period.
    fn on_requested_deadline_missed(&self, reader: &DataReader<T>, status: DeadlineMissedStatus) {}
    /// REQUESTED_INCOMPATIBLE_QOS: a writer was not matched because of
    /// its QoS.
    fn on_requested_incompatible_qos(&self, reader: &DataReader<T>, status: IncompatibleQosStatus) {
    }
    /// SAMPLE_REJECTED: a sample was dropped for the RESOURCE_LIMITS.
    fn on_sample_rejected(&self, reader: &DataReader<T>, status: SampleRejectedStatus) {}
    /// SAMPLE_LOST: samples of a writer never arrived.
    fn on_sample_lost(&self, reader: &DataReader<T>, status: SampleLostStatus) {}
}

/// The listener of a [`DataWriter`] (`DataWriterListener`, DDS 1.4,
/// 2.2.4.3.8): called when one of its statuses changes, as for
/// [`DataReaderListener`]. Each method does nothing unless implemented.
#[allow(unused_variables)]
pub trait DataWriterListener<T>: Send + Sync {
    /// PUBLICATION_MATCHED: a reader was matched or unmatched.
    fn on_publication_matched(&self, writer: &DataWriter<T>, status: MatchedStatus) {}
    /// LIVELINESS_LOST: the writer's lease passed without an assertion.
    fn on_liveliness_lost(&self, writer: &DataWriter<T>, status: LivelinessLostStatus) {}
    /// OFFERED_DEADLINE_MISSED: an instance was not written within the
    /// DEADLINE period.
    fn on_offered_deadline_missed(&self, writer: &DataWriter<T>, status: DeadlineMissedStatus) {}
    /// OFFERED_INCOMPATIBLE_QOS: a reader was not matched because of
    /// this writer's QoS.
    fn on_offered_incompatible_qos(&self, writer: &DataWriter<T>, status: IncompatibleQosStatus) {}
}

/// A data writer of any type, as given to the listener of its publisher
/// or participant (`AnyDataWriter` of the C++ PSM).
#[derive(Clone)]
pub struct AnyDataWriter {
    pub(crate) shared: Arc<Shared>,
    pub(crate) guid: Guid,
}

/// A data reader of any type, as given to the listener of its subscriber
/// or participant (`AnyDataReader` of the C++ PSM).
#[derive(Clone)]
pub struct AnyDataReader {
    pub(crate) shared: Arc<Shared>,
    pub(crate) guid: Guid,
}

macro_rules! any_endpoint {
    ($t:ty, $map:ident) => {
        impl $t {
            /// The GUID of the endpoint.
            pub fn guid(&self) -> Guid {
                self.guid
            }

            /// The name of its topic (empty once it is deleted).
            pub fn topic_name(&self) -> String {
                let g = self.shared.lock_any();
                g.$map
                    .get(&self.guid)
                    .map(|e| e.data.topic_name.clone())
                    .unwrap_or_default()
            }

            /// The name of its type (empty once it is deleted).
            pub fn type_name(&self) -> String {
                let g = self.shared.lock_any();
                g.$map
                    .get(&self.guid)
                    .map(|e| e.data.type_name.clone())
                    .unwrap_or_default()
            }
        }

        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($t))
                    .field("guid", &self.guid)
                    .finish()
            }
        }
    };
}
any_endpoint!(AnyDataWriter, writers);
any_endpoint!(AnyDataReader, readers);

/// The listener of a [`Publisher`](crate::Publisher)
/// (`PublisherListener`, DDS 1.4, 2.2.2.4.3): the statuses of its
/// writers that their own listeners do not take. Each method does nothing
/// unless implemented.
#[allow(unused_variables)]
pub trait PublisherListener: Send + Sync {
    /// PUBLICATION_MATCHED of one of its writers.
    fn on_publication_matched(&self, writer: &AnyDataWriter, status: MatchedStatus) {}
    /// LIVELINESS_LOST of one of its writers.
    fn on_liveliness_lost(&self, writer: &AnyDataWriter, status: LivelinessLostStatus) {}
    /// OFFERED_DEADLINE_MISSED of one of its writers.
    fn on_offered_deadline_missed(&self, writer: &AnyDataWriter, status: DeadlineMissedStatus) {}
    /// OFFERED_INCOMPATIBLE_QOS of one of its writers.
    fn on_offered_incompatible_qos(&self, writer: &AnyDataWriter, status: IncompatibleQosStatus) {}
}

/// The listener of a [`Subscriber`] (`SubscriberListener`, DDS 1.4,
/// 2.2.2.5.6): DATA_ON_READERS, and the statuses of its readers that
/// their own listeners do not take. Each method does nothing unless
/// implemented.
#[allow(unused_variables)]
pub trait SubscriberListener: Send + Sync {
    /// DATA_ON_READERS: some of its readers have new data. Then no
    /// `on_data_available` is called (2.2.4.3.2), and the readers keep
    /// their DATA_AVAILABLE status; [`Subscriber::notify_datareaders`]
    /// calls their listeners.
    fn on_data_on_readers(&self, subscriber: &Subscriber) {}
    /// DATA_AVAILABLE of one of its readers.
    fn on_data_available(&self, reader: &AnyDataReader) {}
    /// SUBSCRIPTION_MATCHED of one of its readers.
    fn on_subscription_matched(&self, reader: &AnyDataReader, status: MatchedStatus) {}
    /// LIVELINESS_CHANGED of one of its readers.
    fn on_liveliness_changed(&self, reader: &AnyDataReader, status: LivelinessChangedStatus) {}
    /// REQUESTED_DEADLINE_MISSED of one of its readers.
    fn on_requested_deadline_missed(&self, reader: &AnyDataReader, status: DeadlineMissedStatus) {}
    /// REQUESTED_INCOMPATIBLE_QOS of one of its readers.
    fn on_requested_incompatible_qos(&self, reader: &AnyDataReader, status: IncompatibleQosStatus) {
    }
    /// SAMPLE_REJECTED of one of its readers.
    fn on_sample_rejected(&self, reader: &AnyDataReader, status: SampleRejectedStatus) {}
    /// SAMPLE_LOST of one of its readers.
    fn on_sample_lost(&self, reader: &AnyDataReader, status: SampleLostStatus) {}
}

/// The listener of a [`Topic`](crate::Topic) (`TopicListener`, DDS 1.4,
/// 2.2.2.3.5). Each method does nothing unless implemented.
#[allow(unused_variables)]
pub trait TopicListener: Send + Sync {
    /// INCONSISTENT_TOPIC: an endpoint of another participant has the
    /// topic's name and a type not consistent with the local one.
    fn on_inconsistent_topic(&self, topic_name: &str, status: InconsistentTopicStatus) {}
}

/// The listener of a [`DomainParticipant`](crate::DomainParticipant)
/// (`DomainParticipantListener`, DDS 1.4, 2.2.2.2.3): what the listeners
/// of its publishers, subscribers, topics, writers and readers do not
/// take. It has the methods of [`PublisherListener`],
/// [`SubscriberListener`] and [`TopicListener`]; implement those
/// (empty `impl` blocks for the ones not needed) and this.
pub trait DomainParticipantListener:
    PublisherListener + SubscriberListener + TopicListener
{
}

/// A publisher, as the participant keeps it: its QoS and listener,
/// shared by its handles and its writers.
pub(crate) struct PublisherEntity {
    pub(crate) qos: PublisherQos,
    pub(crate) listener: Mutex<Option<(Arc<dyn PublisherListener>, StatusMask)>>,
}

/// A subscriber, as the participant keeps it.
pub(crate) struct SubscriberEntity {
    pub(crate) qos: SubscriberQos,
    pub(crate) listener: Mutex<Option<(Arc<dyn SubscriberListener>, StatusMask)>>,
}

impl PublisherEntity {
    pub(crate) fn new(qos: PublisherQos) -> Arc<Self> {
        Arc::new(PublisherEntity {
            qos,
            listener: Mutex::new(None),
        })
    }

    /// Its listener, if it takes `bit`.
    pub(crate) fn listener_for(&self, bit: StatusMask) -> Option<Arc<dyn PublisherListener>> {
        let l = self.listener.lock().unwrap_or_else(|e| e.into_inner());
        l.as_ref()
            .filter(|(_, m)| m.contains(bit))
            .map(|(l, _)| l.clone())
    }
}

impl SubscriberEntity {
    pub(crate) fn new(qos: SubscriberQos) -> Arc<Self> {
        Arc::new(SubscriberEntity {
            qos,
            listener: Mutex::new(None),
        })
    }

    /// Its listener, if it takes `bit`.
    pub(crate) fn listener_for(&self, bit: StatusMask) -> Option<Arc<dyn SubscriberListener>> {
        let l = self.listener.lock().unwrap_or_else(|e| e.into_inner());
        l.as_ref()
            .filter(|(_, m)| m.contains(bit))
            .map(|(l, _)| l.clone())
    }
}
