// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import java.util.Collection;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.DomainEntity;
import org.omg.dds.core.Duration;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.topic.Topic;
import org.omg.dds.topic.TopicQos;

/** Makes DataWriters. Closing it closes them. */
public interface Publisher extends DomainEntity<PublisherListener, PublisherQos> {
    public <TYPE> DataWriter<TYPE> createDataWriter(Topic<TYPE> topic);

    public <TYPE> DataWriter<TYPE> createDataWriter(
            Topic<TYPE> topic,
            DataWriterQos qos,
            DataWriterListener<TYPE> listener,
            Collection<Class<? extends Status>> statuses);

    public <TYPE> DataWriter<TYPE> createDataWriter(Topic<TYPE> topic, DataWriterQos qos);

    /** A writer of this publisher for the topic, or null. */
    public <TYPE> DataWriter<TYPE> lookupDataWriter(String topicName);

    public <TYPE> DataWriter<TYPE> lookupDataWriter(Topic<TYPE> topic);

    public void closeContainedEntities();

    /** Until every reliable reader of every writer acknowledged what was
     * written. */
    public void waitForAcknowledgments(Duration maxWait) throws TimeoutException;

    public void waitForAcknowledgments(long maxWait, TimeUnit unit) throws TimeoutException;

    public DataWriterQos getDefaultDataWriterQos();

    public void setDefaultDataWriterQos(DataWriterQos qos);

    /** dwQos with the policies of tQos that apply to writers. */
    public DataWriterQos copyFromTopicQos(DataWriterQos dwQos, TopicQos tQos);

    @Override
    public StatusCondition<Publisher> getStatusCondition();

    @Override
    public DomainParticipant getParent();
}
