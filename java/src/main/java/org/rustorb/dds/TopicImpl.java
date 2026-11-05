// Topic, on rdds_topic.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.Collection;
import java.util.Collections;
import java.util.Set;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.InconsistentTopicStatus;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.topic.Topic;
import org.omg.dds.topic.TopicDescription;
import org.omg.dds.topic.TopicListener;
import org.omg.dds.topic.TopicQos;
import org.omg.dds.type.TypeSupport;

final class TopicImpl<T> extends EntityBase<TopicListener<T>, TopicQos> implements Topic<T> {
    final ParticipantImpl participant;
    private final String name;
    final TypeSupportImpl<T> type;
    private final TopicQos qos;

    TopicImpl(ParticipantImpl participant, String name, TypeSupportImpl<T> type, TopicQos qos) {
        super(participant.env, "Topic " + name, make(participant.env, (arena, out) -> {
            TypeRef t = type.codec.getType();
            return (int) Ffi.rdds_participant_create_topic.invokeExact(participant.handle(),
                    arena.allocateFrom(name).address(), t.handle(), out);
        }));
        this.participant = participant;
        this.name = name;
        this.type = type;
        this.qos = qos;
    }

    Codec<T> codec() {
        return type.codec;
    }

    @Override
    public TypeSupport<T> getTypeSupport() {
        return type;
    }

    @SuppressWarnings("unchecked")
    @Override
    public <OTHER> TopicImpl<OTHER> cast() {
        return (TopicImpl<OTHER>) this;
    }

    /** The name of the type as the participant registered it: the
     * codec's IDL name (rustorb-dds registers types under it). */
    @Override
    public String getTypeName() {
        return type.codec.getTypeName();
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public DomainParticipant getParent() {
        return participant;
    }

    @Override
    public InconsistentTopicStatus getInconsistentTopicStatus() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment s = arena.allocate(Statuses.COUNT_SIZE, 4);
            call(env, () -> (int) Ffi.rdds_topic_inconsistent_topic_status.invokeExact(handle(), s.address()));
            return new Statuses.InconsistentTopic(env, s.get(java.lang.foreign.ValueLayout.JAVA_INT, 0),
                    s.get(java.lang.foreign.ValueLayout.JAVA_INT, 4));
        }
    }

    @Override
    public StatusCondition<Topic<T>> getStatusCondition() {
        throw new UnsupportedOperationException("rustorb-dds has no status condition of a topic yet");
    }

    @Override
    public TopicListener<T> getListener() {
        return null;
    }

    @Override
    public void setListener(TopicListener<T> listener) {
        ParticipantImpl.unsupportedListener(listener, "topic");
    }

    @Override
    public void setListener(TopicListener<T> listener, Collection<Class<? extends Status>> statuses) {
        ParticipantImpl.unsupportedListener(listener, "topic");
    }

    @Override
    public TopicQos getQos() {
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
        long h = takeHandle();
        if (h != 0) {
            participant.topics.remove(this);
            try {
                Ffi.rdds_topic_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    /** The topic, if it is one of this implementation. */
    @SuppressWarnings("unchecked")
    static <T> TopicImpl<T> of(TopicDescription<T> t) {
        if (t instanceof TopicImpl<?> i) {
            return (TopicImpl<T>) i;
        }
        throw new IllegalArgumentException("a topic of another DDS implementation: " + t);
    }

    @Override
    public String toString() {
        return "Topic(" + name + ", " + getTypeName() + ")";
    }
}
