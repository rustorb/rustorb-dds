// Subscriber, on rdds_subscriber.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.lang.foreign.MemorySegment;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.DataReaderListener;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.SubscriberListener;
import org.omg.dds.sub.SubscriberQos;
import org.omg.dds.topic.TopicDescription;
import org.omg.dds.topic.TopicQos;

final class SubscriberImpl extends EntityBase<SubscriberListener, SubscriberQos> implements Subscriber {
    final ParticipantImpl participant;
    private final SubscriberQos qos;
    final List<ReaderImpl<?>> readers = new CopyOnWriteArrayList<>();
    private volatile DataReaderQos defaultReaderQos;

    SubscriberImpl(ParticipantImpl participant, SubscriberQos qos) {
        super(participant.env, "Subscriber", make(participant.env, (arena, out) -> {
            MemorySegment q = Qos.GroupQos.toNative(arena, qos.getPartition());
            return (int) Ffi.rdds_participant_create_subscriber.invokeExact(participant.handle(), q.address(), out);
        }));
        this.participant = participant;
        this.qos = qos;
        this.defaultReaderQos = Qos.ReaderQos.defaults(env);
    }

    @Override
    public <TYPE> DataReader<TYPE> createDataReader(TopicDescription<TYPE> topic) {
        return createDataReader(topic, null, null, null);
    }

    @Override
    public <TYPE> DataReader<TYPE> createDataReader(TopicDescription<TYPE> topic, DataReaderQos qos) {
        return createDataReader(topic, qos, null, null);
    }

    @Override
    public <TYPE> DataReader<TYPE> createDataReader(TopicDescription<TYPE> topic, DataReaderQos qos,
            DataReaderListener<TYPE> listener, Collection<Class<? extends Status>> statuses) {
        ContentFilteredTopicImpl<TYPE> cft = topic instanceof ContentFilteredTopicImpl<?> c
                ? (ContentFilteredTopicImpl<TYPE>) c.<TYPE>cast()
                : null;
        TopicImpl<TYPE> t = cft != null ? cft.related : TopicImpl.of(topic);
        if (t.participant != participant) {
            throw Errors.precondition(env, "the topic belongs to another participant");
        }
        ReaderImpl<TYPE> r = ReaderImpl.create(this, t, cft, (Qos.ReaderQos) (qos == null ? defaultReaderQos : qos),
                listener, StatusKinds.mask(statuses));
        readers.add(r);
        GroupListeners.created(r.getInstanceHandle());
        return r;
    }

    @SuppressWarnings("unchecked")
    @Override
    public <TYPE> DataReader<TYPE> lookupDataReader(String topicName) {
        for (ReaderImpl<?> r : readers) {
            if (r.getTopicDescription().getName().equals(topicName)) {
                return (DataReader<TYPE>) r;
            }
        }
        return null;
    }

    @Override
    public <TYPE> DataReader<TYPE> lookupDataReader(TopicDescription<TYPE> topic) {
        return lookupDataReader(topic.getName());
    }

    @Override
    public void closeContainedEntities() {
        for (ReaderImpl<?> r : readers) {
            r.close();
        }
    }

    @Override
    public DataReaderQos getDefaultDataReaderQos() {
        return defaultReaderQos;
    }

    @Override
    public void setDefaultDataReaderQos(DataReaderQos qos) {
        defaultReaderQos = qos == null ? Qos.ReaderQos.defaults(env) : qos;
    }

    @Override
    public DataReaderQos copyFromTopicQos(DataReaderQos drQos, TopicQos tQos) {
        return ((Qos.ReaderQos) drQos).withTopic(tQos);
    }

    @Override
    public StatusCondition<Subscriber> getStatusCondition() {
        throw new UnsupportedOperationException("rustorb-dds has no status condition of a subscriber yet");
    }

    @Override
    public DomainParticipant getParent() {
        return participant;
    }

    @Override
    public DataState createDataState() {
        return new DataStateImpl(env);
    }

    private volatile SubscriberListener listener;

    @Override
    public SubscriberListener getListener() {
        return listener;
    }

    @Override
    public void setListener(SubscriberListener listener) {
        setListener(listener, null);
    }

    @Override
    public void setListener(SubscriberListener listener, Collection<Class<? extends Status>> statuses) {
        GroupListeners.set(participant, listener, null, StatusKinds.mask(statuses),
                (t, m) -> (int) Ffi.rdds_subscriber_set_listener.invokeExact(handle(), t, m));
        this.listener = listener;
    }

    @Override
    public void notifyDataReaders() {
        call(env, () -> (int) Ffi.rdds_subscriber_notify_datareaders.invokeExact(handle()));
    }

    @Override
    public SubscriberQos getQos() {
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
            participant.subscribers.remove(this);
            try {
                Ffi.rdds_subscriber_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }
}
