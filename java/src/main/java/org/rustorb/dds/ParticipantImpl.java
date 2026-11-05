// DomainParticipant and its factory, on rdds_participant.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableTime;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.Time;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.domain.DomainParticipantFactory;
import org.omg.dds.domain.DomainParticipantListener;
import org.omg.dds.domain.DomainParticipantQos;
import org.omg.dds.pub.Publisher;
import org.omg.dds.pub.PublisherListener;
import org.omg.dds.pub.PublisherQos;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.SubscriberListener;
import org.omg.dds.sub.SubscriberQos;
import org.omg.dds.topic.Topic;
import org.omg.dds.topic.TopicDescription;
import org.omg.dds.topic.TopicListener;
import org.omg.dds.topic.TopicQos;
import org.omg.dds.type.TypeSupport;

final class ParticipantImpl extends EntityBase<DomainParticipantListener, DomainParticipantQos>
        implements DomainParticipant {
    static final int LOCALHOST = 1;

    final Factory factory;
    private final int domainId;
    private final DomainParticipantQos qos;
    final List<PublisherImpl> publishers = new CopyOnWriteArrayList<>();
    final List<SubscriberImpl> subscribers = new CopyOnWriteArrayList<>();
    final List<TopicImpl<?>> topics = new CopyOnWriteArrayList<>();
    private volatile PublisherQos defaultPublisherQos;
    private volatile SubscriberQos defaultSubscriberQos;
    private volatile TopicQos defaultTopicQos;

    ParticipantImpl(Factory factory, int domainId, DomainParticipantQos qos, int flags) {
        super(factory.env, "DomainParticipant", make(factory.env, (arena, out) -> {
            MemorySegment q = ((Qos.ParticipantQos) qos).toNative(arena);
            return (int) Ffi.rdds_participant_new.invokeExact(domainId, q.address(), flags, out);
        }));
        this.factory = factory;
        this.domainId = domainId;
        this.qos = qos;
        this.defaultPublisherQos = Qos.PubQos.defaults(env);
        this.defaultSubscriberQos = Qos.SubQos.defaults(env);
        this.defaultTopicQos = Qos.TopicQosImpl.defaults(env);
    }

    static void unsupportedListener(Object listener, String what) {
        if (listener != null) {
            throw new UnsupportedOperationException("rustorb-dds has no " + what + " listeners yet");
        }
    }

    @Override
    public DomainParticipantListener getListener() {
        return null;
    }

    @Override
    public void setListener(DomainParticipantListener listener) {
        unsupportedListener(listener, "participant");
    }

    @Override
    public void setListener(DomainParticipantListener listener, Collection<Class<? extends Status>> statuses) {
        unsupportedListener(listener, "participant");
    }

    @Override
    public DomainParticipantQos getQos() {
        return qos;
    }

    @Override
    public StatusCondition<DomainParticipant> getStatusCondition() {
        throw new UnsupportedOperationException("rustorb-dds has no status condition of a participant yet");
    }

    @Override
    public Set<Class<? extends Status>> getStatusChanges() {
        return Collections.emptySet();
    }

    @Override
    public InstanceHandle getInstanceHandle() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment g = arena.allocate(Ffi.HANDLE);
            call(env, () -> (int) Ffi.rdds_participant_guid.invokeExact(handle(), g.address()));
            return HandleImpl.get(env, g, 0);
        }
    }

    @Override
    public void close() {
        closeContainedEntities();
        long h = takeHandle();
        if (h != 0) {
            factory.participants.remove(this);
            try {
                Ffi.rdds_participant_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    @Override
    public Publisher createPublisher() {
        return createPublisher(defaultPublisherQos, null, null);
    }

    @Override
    public Publisher createPublisher(PublisherQos qos) {
        return createPublisher(qos, null, null);
    }

    @Override
    public Publisher createPublisher(
            PublisherQos qos, PublisherListener listener, Collection<Class<? extends Status>> statuses) {
        unsupportedListener(listener, "publisher");
        PublisherImpl p = new PublisherImpl(this, qos == null ? defaultPublisherQos : qos);
        publishers.add(p);
        return p;
    }

    @Override
    public Subscriber createSubscriber() {
        return createSubscriber(defaultSubscriberQos, null, null);
    }

    @Override
    public Subscriber createSubscriber(SubscriberQos qos) {
        return createSubscriber(qos, null, null);
    }

    @Override
    public Subscriber createSubscriber(
            SubscriberQos qos, SubscriberListener listener, Collection<Class<? extends Status>> statuses) {
        unsupportedListener(listener, "subscriber");
        SubscriberImpl s = new SubscriberImpl(this, qos == null ? defaultSubscriberQos : qos);
        subscribers.add(s);
        return s;
    }

    @Override
    public <TYPE> Topic<TYPE> createTopic(String topicName, Class<TYPE> type) {
        return createTopic(topicName, type, null, null, null);
    }

    @Override
    public <TYPE> Topic<TYPE> createTopic(String topicName, Class<TYPE> type, TopicQos qos,
            TopicListener<TYPE> listener, Collection<Class<? extends Status>> statuses) {
        return createTopic(topicName, TypeSupport.newTypeSupport(type, env), qos, listener, statuses);
    }

    @Override
    public <TYPE> Topic<TYPE> createTopic(String topicName, TypeSupport<TYPE> type) {
        return createTopic(topicName, type, null, null, null);
    }

    @Override
    public <TYPE> Topic<TYPE> createTopic(String topicName, TypeSupport<TYPE> type, TopicQos qos,
            TopicListener<TYPE> listener, Collection<Class<? extends Status>> statuses) {
        unsupportedListener(listener, "topic");
        if (!(type instanceof TypeSupportImpl<TYPE> ts)) {
            throw new IllegalArgumentException("a TypeSupport of another DDS implementation");
        }
        TopicImpl<TYPE> t = new TopicImpl<>(this, topicName, ts, qos == null ? defaultTopicQos : qos);
        topics.add(t);
        return t;
    }

    @SuppressWarnings("unchecked")
    @Override
    public <TYPE> TopicDescription<TYPE> lookupTopicDescription(String name) {
        for (TopicImpl<?> t : topics) {
            if (t.getName().equals(name)) {
                return (TopicDescription<TYPE>) t;
            }
        }
        return null;
    }

    @Override
    public void closeContainedEntities() {
        for (PublisherImpl p : publishers) {
            p.close();
        }
        for (SubscriberImpl s : subscribers) {
            s.close();
        }
        for (TopicImpl<?> t : topics) {
            t.close();
        }
    }

    @Override
    public int getDomainId() {
        return domainId;
    }

    @Override
    public void assertLiveliness() {
        call(env, () -> (int) Ffi.rdds_participant_assert_liveliness.invokeExact(handle()));
    }

    @Override
    public PublisherQos getDefaultPublisherQos() {
        return defaultPublisherQos;
    }

    @Override
    public void setDefaultPublisherQos(PublisherQos qos) {
        defaultPublisherQos = qos == null ? Qos.PubQos.defaults(env) : qos;
    }

    @Override
    public SubscriberQos getDefaultSubscriberQos() {
        return defaultSubscriberQos;
    }

    @Override
    public void setDefaultSubscriberQos(SubscriberQos qos) {
        defaultSubscriberQos = qos == null ? Qos.SubQos.defaults(env) : qos;
    }

    @Override
    public TopicQos getDefaultTopicQos() {
        return defaultTopicQos;
    }

    @Override
    public void setDefaultTopicQos(TopicQos qos) {
        defaultTopicQos = qos == null ? Qos.TopicQosImpl.defaults(env) : qos;
    }

    @Override
    public ModifiableTime getCurrentTime(ModifiableTime currentTime) {
        currentTime.copyFrom(TimeImpl.now(env));
        return currentTime;
    }

    @Override
    public Time getCurrentTime() {
        return TimeImpl.now(env);
    }

    /** How many other participants are known (rustorb extension). */
    long discoveredParticipants() {
        try {
            return (long) Ffi.rdds_participant_discovered_participants.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    @Override
    public String toString() {
        return "DomainParticipant(domain " + domainId + ")";
    }

    /** The DomainParticipantFactory of a RustorbServiceEnvironment. */
    static final class Factory extends DomainParticipantFactory {
        final ServiceEnvironment env;
        final List<ParticipantImpl> participants = new CopyOnWriteArrayList<>();
        private volatile DomainParticipantQos defaultQos;

        Factory(ServiceEnvironment env) {
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public DomainParticipant createParticipant() {
            return createParticipant(0);
        }

        @Override
        public DomainParticipant createParticipant(int domainId) {
            return createParticipant(domainId, null, null, null);
        }

        @Override
        public DomainParticipant createParticipant(int domainId, DomainParticipantQos qos,
                DomainParticipantListener listener, Collection<Class<? extends Status>> statuses) {
            return create(domainId, qos, listener, 0);
        }

        ParticipantImpl create(int domainId, DomainParticipantQos qos, DomainParticipantListener listener,
                int flags) {
            unsupportedListener(listener, "participant");
            if (domainId < 0) {
                throw new IllegalArgumentException("a negative domain id");
            }
            ParticipantImpl p = new ParticipantImpl(this, domainId, qos == null ? getDefaultParticipantQos() : qos,
                    flags);
            participants.add(p);
            return p;
        }

        @Override
        public DomainParticipant lookupParticipant(int domainId) {
            for (ParticipantImpl p : participants) {
                if (p.getDomainId() == domainId) {
                    return p;
                }
            }
            return null;
        }

        @Override
        public DomainParticipantQos getDefaultParticipantQos() {
            DomainParticipantQos q = defaultQos;
            if (q == null) {
                q = Qos.ParticipantQos.defaults(env);
                defaultQos = q;
            }
            return q;
        }

        @Override
        public void setDefaultParticipantQos(DomainParticipantQos qos) {
            defaultQos = qos;
        }
    }
}
