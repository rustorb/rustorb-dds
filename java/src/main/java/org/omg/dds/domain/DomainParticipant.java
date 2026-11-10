// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.domain;

import java.util.Collection;
import org.omg.dds.core.Entity;
import org.omg.dds.core.ModifiableTime;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.Time;
import org.omg.dds.core.status.Status;
import org.omg.dds.pub.Publisher;
import org.omg.dds.pub.PublisherListener;
import org.omg.dds.pub.PublisherQos;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.SubscriberListener;
import org.omg.dds.sub.SubscriberQos;
import org.omg.dds.topic.ContentFilteredTopic;
import org.omg.dds.topic.Topic;
import org.omg.dds.topic.TopicDescription;
import org.omg.dds.topic.TopicListener;
import org.omg.dds.topic.TopicQos;
import org.omg.dds.type.TypeSupport;

/** A participant in a DDS domain: makes the publishers, subscribers and
 * topics. Closing it closes them. */
public interface DomainParticipant extends Entity<DomainParticipantListener, DomainParticipantQos> {
    public Publisher createPublisher();

    public Publisher createPublisher(PublisherQos qos);

    public Publisher createPublisher(
            PublisherQos qos, PublisherListener listener, Collection<Class<? extends Status>> statuses);

    public Subscriber createSubscriber();

    public Subscriber createSubscriber(SubscriberQos qos);

    public Subscriber createSubscriber(
            SubscriberQos qos, SubscriberListener listener, Collection<Class<? extends Status>> statuses);

    /** A topic of the type of a class rustorb-idl generated code for. */
    public <TYPE> Topic<TYPE> createTopic(String topicName, Class<TYPE> type);

    public <TYPE> Topic<TYPE> createTopic(
            String topicName,
            Class<TYPE> type,
            TopicQos qos,
            TopicListener<TYPE> listener,
            Collection<Class<? extends Status>> statuses);

    public <TYPE> Topic<TYPE> createTopic(String topicName, TypeSupport<TYPE> type);

    public <TYPE> Topic<TYPE> createTopic(
            String topicName,
            TypeSupport<TYPE> type,
            TopicQos qos,
            TopicListener<TYPE> listener,
            Collection<Class<? extends Status>> statuses);

    /** A topic of this participant by name, or null. */
    public <TYPE> TopicDescription<TYPE> lookupTopicDescription(String name);

    public <TYPE> ContentFilteredTopic<TYPE> createContentFilteredTopic(
            String name,
            Topic<? extends TYPE> relatedTopic,
            String filterExpression,
            java.util.List<String> expressionParameters);

    public <TYPE> ContentFilteredTopic<TYPE> createContentFilteredTopic(
            String name,
            Topic<? extends TYPE> relatedTopic,
            String filterExpression,
            String... expressionParameters);

    public void closeContainedEntities();

    public int getDomainId();

    public void assertLiveliness();

    public PublisherQos getDefaultPublisherQos();

    public void setDefaultPublisherQos(PublisherQos qos);

    public SubscriberQos getDefaultSubscriberQos();

    public void setDefaultSubscriberQos(SubscriberQos qos);

    public TopicQos getDefaultTopicQos();

    public void setDefaultTopicQos(TopicQos qos);

    public ModifiableTime getCurrentTime(ModifiableTime currentTime);

    public Time getCurrentTime();

    @Override
    public StatusCondition<DomainParticipant> getStatusCondition();
}
