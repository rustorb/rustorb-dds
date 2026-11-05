// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.util.Collection;
import java.util.Set;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.DomainEntity;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.topic.TopicDescription;
import org.omg.dds.topic.TopicQos;

/** Makes DataReaders. Closing it closes them. */
public interface Subscriber extends DomainEntity<SubscriberListener, SubscriberQos> {
    public <TYPE> DataReader<TYPE> createDataReader(TopicDescription<TYPE> topic);

    public <TYPE> DataReader<TYPE> createDataReader(
            TopicDescription<TYPE> topic,
            DataReaderQos qos,
            DataReaderListener<TYPE> listener,
            Collection<Class<? extends Status>> statuses);

    public <TYPE> DataReader<TYPE> createDataReader(TopicDescription<TYPE> topic, DataReaderQos qos);

    /** A reader of this subscriber for the topic, or null. */
    public <TYPE> DataReader<TYPE> lookupDataReader(String topicName);

    public <TYPE> DataReader<TYPE> lookupDataReader(TopicDescription<TYPE> topicName);

    public void closeContainedEntities();

    public DataReaderQos getDefaultDataReaderQos();

    public void setDefaultDataReaderQos(DataReaderQos qos);

    /** drQos with the policies of tQos that apply to readers. */
    public DataReaderQos copyFromTopicQos(DataReaderQos drQos, TopicQos tQos);

    @Override
    public StatusCondition<Subscriber> getStatusCondition();

    @Override
    public DomainParticipant getParent();

    /** A DataState with no states yet: add them with its with methods.
     * A kind with no state selects any state of that kind. */
    public DataState createDataState();

    /** Sets of sample, view and instance states, to select samples. */
    public static interface DataState extends DDSObject, Cloneable {
        public Set<SampleState> getSampleStates();

        public Set<ViewState> getViewStates();

        public Set<InstanceState> getInstanceStates();

        /** Adds the state. */
        public DataState with(SampleState state);

        public DataState with(ViewState state);

        public DataState with(InstanceState state);

        public DataState withAnySampleState();

        public DataState withAnyViewState();

        public DataState withAnyInstanceState();

        public DataState withNotAliveInstanceStates();

        public DataState clone();
    }
}
