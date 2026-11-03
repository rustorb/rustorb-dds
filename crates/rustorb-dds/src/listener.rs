//! Listeners of data readers and data writers (DDS 1.4, 2.2.4.3).

use rustorb_typesupport::TopicType;

use crate::publisher::DataWriter;
use crate::status::{
    DeadlineMissedStatus, IncompatibleQosStatus, LivelinessChangedStatus, LivelinessLostStatus,
    MatchedStatus, SampleLostStatus, SampleRejectedStatus,
};
use crate::subscriber::DataReader;

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
pub trait DataReaderListener<T: TopicType>: Send + Sync {
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
pub trait DataWriterListener<T: TopicType>: Send + Sync {
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
