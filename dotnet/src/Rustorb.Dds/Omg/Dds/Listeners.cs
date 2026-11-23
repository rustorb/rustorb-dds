// The listeners (DDS 1.4, 2.2.4.3). Every method has an empty default, so a
// listener implements only what it wants. They are called on a thread of
// the library, or, for a reader of the same participant as the writer,
// inside the Write that wrote. A status an entity's listener does not take
// goes to its publisher's or subscriber's, then to its participant's.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using Omg.Dds.Core.Status;
using Omg.Dds.Pub;
using Omg.Dds.Sub;
using Omg.Dds.Topic;

namespace Omg.Dds.Pub
{
    public interface IDataWriterListener<T>
    {
        void OnOfferedDeadlineMissed(DataWriter<T> writer, OfferedDeadlineMissedStatus status) { }
        void OnOfferedIncompatibleQos(DataWriter<T> writer, OfferedIncompatibleQosStatus status) { }
        void OnLivelinessLost(DataWriter<T> writer, LivelinessLostStatus status) { }
        void OnPublicationMatched(DataWriter<T> writer, PublicationMatchedStatus status) { }
    }

    /// <summary>The listener of a publisher: the statuses of its writers.</summary>
    public interface IPublisherListener
    {
        void OnOfferedDeadlineMissed(IDataWriter writer, OfferedDeadlineMissedStatus status) { }
        void OnOfferedIncompatibleQos(IDataWriter writer, OfferedIncompatibleQosStatus status) { }
        void OnLivelinessLost(IDataWriter writer, LivelinessLostStatus status) { }
        void OnPublicationMatched(IDataWriter writer, PublicationMatchedStatus status) { }
    }
}

namespace Omg.Dds.Sub
{
    public interface IDataReaderListener<T>
    {
        void OnRequestedDeadlineMissed(DataReader<T> reader, RequestedDeadlineMissedStatus status) { }
        void OnRequestedIncompatibleQos(DataReader<T> reader, RequestedIncompatibleQosStatus status) { }
        void OnSampleRejected(DataReader<T> reader, SampleRejectedStatus status) { }
        void OnLivelinessChanged(DataReader<T> reader, LivelinessChangedStatus status) { }
        void OnDataAvailable(DataReader<T> reader) { }
        void OnSubscriptionMatched(DataReader<T> reader, SubscriptionMatchedStatus status) { }
        void OnSampleLost(DataReader<T> reader, SampleLostStatus status) { }
    }

    /// <summary>The listener of a subscriber: the statuses of its readers,
    /// and DATA_ON_READERS.</summary>
    public interface ISubscriberListener
    {
        void OnRequestedDeadlineMissed(IDataReader reader, RequestedDeadlineMissedStatus status) { }
        void OnRequestedIncompatibleQos(IDataReader reader, RequestedIncompatibleQosStatus status) { }
        void OnSampleRejected(IDataReader reader, SampleRejectedStatus status) { }
        void OnLivelinessChanged(IDataReader reader, LivelinessChangedStatus status) { }
        void OnDataAvailable(IDataReader reader) { }
        void OnSubscriptionMatched(IDataReader reader, SubscriptionMatchedStatus status) { }
        void OnSampleLost(IDataReader reader, SampleLostStatus status) { }
        /// <summary>New data on readers of the subscriber; NotifyDataReaders
        /// calls their OnDataAvailable.</summary>
        void OnDataOnReaders(Subscriber subscriber) { }
    }
}

namespace Omg.Dds.Topic
{
    public interface ITopicListener
    {
        void OnInconsistentTopic(ITopic topic, InconsistentTopicStatus status) { }
    }
}

namespace Omg.Dds.Domain
{
    /// <summary>The listener of a participant: what its entities' listeners do not take.</summary>
    public interface IDomainParticipantListener : IPublisherListener, ISubscriberListener, ITopicListener
    {
    }
}
