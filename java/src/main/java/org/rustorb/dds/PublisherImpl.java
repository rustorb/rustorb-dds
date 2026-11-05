// Publisher, on rdds_publisher.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.lang.foreign.MemorySegment;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.DataWriterListener;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.pub.Publisher;
import org.omg.dds.pub.PublisherListener;
import org.omg.dds.pub.PublisherQos;
import org.omg.dds.topic.Topic;
import org.omg.dds.topic.TopicQos;

final class PublisherImpl extends EntityBase<PublisherListener, PublisherQos> implements Publisher {
    final ParticipantImpl participant;
    private final PublisherQos qos;
    final List<WriterImpl<?>> writers = new CopyOnWriteArrayList<>();
    private volatile DataWriterQos defaultWriterQos;

    PublisherImpl(ParticipantImpl participant, PublisherQos qos) {
        super(participant.env, "Publisher", make(participant.env, (arena, out) -> {
            MemorySegment q = Qos.GroupQos.toNative(arena, qos.getPartition());
            return (int) Ffi.rdds_participant_create_publisher.invokeExact(participant.handle(), q.address(), out);
        }));
        this.participant = participant;
        this.qos = qos;
        this.defaultWriterQos = Qos.WriterQos.defaults(env);
    }

    @Override
    public <TYPE> DataWriter<TYPE> createDataWriter(Topic<TYPE> topic) {
        return createDataWriter(topic, null, null, null);
    }

    @Override
    public <TYPE> DataWriter<TYPE> createDataWriter(Topic<TYPE> topic, DataWriterQos qos) {
        return createDataWriter(topic, qos, null, null);
    }

    @Override
    public <TYPE> DataWriter<TYPE> createDataWriter(Topic<TYPE> topic, DataWriterQos qos,
            DataWriterListener<TYPE> listener, Collection<Class<? extends Status>> statuses) {
        TopicImpl<TYPE> t = TopicImpl.of(topic);
        if (t.participant != participant) {
            throw Errors.precondition(env, "the topic belongs to another participant");
        }
        WriterImpl<TYPE> w = WriterImpl.create(this, t, (Qos.WriterQos) (qos == null ? defaultWriterQos : qos),
                listener, StatusKinds.mask(statuses));
        writers.add(w);
        return w;
    }

    @SuppressWarnings("unchecked")
    @Override
    public <TYPE> DataWriter<TYPE> lookupDataWriter(String topicName) {
        for (WriterImpl<?> w : writers) {
            if (w.getTopic().getName().equals(topicName)) {
                return (DataWriter<TYPE>) w;
            }
        }
        return null;
    }

    @Override
    public <TYPE> DataWriter<TYPE> lookupDataWriter(Topic<TYPE> topic) {
        return lookupDataWriter(topic.getName());
    }

    @Override
    public void closeContainedEntities() {
        for (WriterImpl<?> w : writers) {
            w.close();
        }
    }

    @Override
    public void waitForAcknowledgments(Duration maxWait) throws TimeoutException {
        long deadline = System.nanoTime() + Math.min(DurationImpl.nanos(maxWait), Long.MAX_VALUE / 2);
        boolean infinite = maxWait.isInfinite();
        for (WriterImpl<?> w : writers) {
            long left = infinite ? DurationImpl.INFINITE : Math.max(0, deadline - System.nanoTime());
            w.waitForAcknowledgments(new DurationImpl(env, left));
        }
    }

    @Override
    public void waitForAcknowledgments(long maxWait, TimeUnit unit) throws TimeoutException {
        waitForAcknowledgments(DurationImpl.of(env, maxWait, unit));
    }

    @Override
    public DataWriterQos getDefaultDataWriterQos() {
        return defaultWriterQos;
    }

    @Override
    public void setDefaultDataWriterQos(DataWriterQos qos) {
        defaultWriterQos = qos == null ? Qos.WriterQos.defaults(env) : qos;
    }

    @Override
    public DataWriterQos copyFromTopicQos(DataWriterQos dwQos, TopicQos tQos) {
        return ((Qos.WriterQos) dwQos).withTopic(tQos);
    }

    @Override
    public StatusCondition<Publisher> getStatusCondition() {
        throw new UnsupportedOperationException("rustorb-dds has no status condition of a publisher yet");
    }

    @Override
    public DomainParticipant getParent() {
        return participant;
    }

    @Override
    public PublisherListener getListener() {
        return null;
    }

    @Override
    public void setListener(PublisherListener listener) {
        ParticipantImpl.unsupportedListener(listener, "publisher");
    }

    @Override
    public void setListener(PublisherListener listener, Collection<Class<? extends Status>> statuses) {
        ParticipantImpl.unsupportedListener(listener, "publisher");
    }

    @Override
    public PublisherQos getQos() {
        return qos;
    }

    @Override
    public Set<Class<? extends Status>> getStatusChanges() {
        return Collections.emptySet();
    }

    @Override
    public InstanceHandle getInstanceHandle() {
        return HandleImpl.nil(env);
    }

    @Override
    public void close() {
        closeContainedEntities();
        long h = takeHandle();
        if (h != 0) {
            participant.publishers.remove(this);
            try {
                Ffi.rdds_publisher_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }
}
