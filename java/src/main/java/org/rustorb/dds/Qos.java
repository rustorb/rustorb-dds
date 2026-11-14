// The entity QoS of the PSM: immutable maps from policy class to policy,
// and their C structs (rdds_*_qos).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;
import static java.lang.foreign.ValueLayout.JAVA_SHORT;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.AbstractMap;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.omg.dds.core.EntityQos;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.policy.DataRepresentation;
import org.omg.dds.core.policy.Deadline;
import org.omg.dds.core.policy.Durability;
import org.omg.dds.core.policy.History;
import org.omg.dds.core.policy.Liveliness;
import org.omg.dds.core.policy.Partition;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.policy.Property;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.Reliability;
import org.omg.dds.core.policy.ResourceLimits;
import org.omg.dds.core.policy.UserData;
import org.omg.dds.core.policy.WriterDataLifecycle;
import org.omg.dds.domain.DomainParticipantQos;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.pub.PublisherQos;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.sub.SubscriberQos;
import org.omg.dds.topic.TopicQos;

final class Qos {
    private Qos() {}

    /** The policies of an entity QoS, by the class of their PSM interface. */
    abstract static class Base<P extends QosPolicy, SELF extends Base<P, SELF>>
            extends AbstractMap<Class<? extends P>, P> implements EntityQos<P> {
        private static final long serialVersionUID = 1L;
        final transient ServiceEnvironment env;
        final Map<Class<? extends P>, P> policies;

        Base(ServiceEnvironment env, Map<Class<? extends P>, P> policies) {
            this.env = env;
            this.policies = Collections.unmodifiableMap(new LinkedHashMap<>(policies));
        }

        abstract SELF make(Map<Class<? extends P>, P> policies);

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public Set<Entry<Class<? extends P>, P>> entrySet() {
            return policies.entrySet();
        }

        @Override
        public <POLICY extends P> POLICY get(Class<POLICY> id) {
            return id.cast(policies.get(id));
        }

        @SuppressWarnings("unchecked")
        @Override
        public SELF withPolicy(P policy) {
            if (!(policy instanceof Policies.Base b)) {
                throw new IllegalArgumentException("a policy of another DDS implementation: " + policy);
            }
            Class<? extends P> id = (Class<? extends P>) b.id();
            if (!policies.containsKey(id)) {
                throw new UnsupportedOperationException(
                        id.getSimpleName() + " is not a policy rustorb-dds supports for this entity");
            }
            Map<Class<? extends P>, P> m = new LinkedHashMap<>(policies);
            m.put(id, policy);
            return make(m);
        }

        @SafeVarargs
        @Override
        public final SELF withPolicies(P... policy) {
            SELF q = make(policies);
            for (P p : policy) {
                q = q.withPolicy(p);
            }
            return q;
        }

        @Override
        public PolicyFactory getPolicyFactory() {
            return env.getSPI().getPolicyFactory();
        }
    }

    /** A map of the policies given. */
    @SafeVarargs
    static <P extends QosPolicy> Map<Class<? extends P>, P> map(P... ps) {
        Map<Class<? extends P>, P> m = new LinkedHashMap<>();
        for (P p : ps) {
            @SuppressWarnings("unchecked")
            Class<? extends P> id = (Class<? extends P>) ((Policies.Base) p).id();
            m.put(id, p);
        }
        return m;
    }

    static final class ParticipantQos extends Base<QosPolicy.ForDomainParticipant, ParticipantQos>
            implements DomainParticipantQos {
        private static final long serialVersionUID = 1L;

        ParticipantQos(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForDomainParticipant>,
                QosPolicy.ForDomainParticipant> p) {
            super(env, p);
        }

        static ParticipantQos defaults(ServiceEnvironment env) {
            PolicyFactory f = env.getSPI().getPolicyFactory();
            return new ParticipantQos(env, map(f.UserData(), f.Property()));
        }

        @Override
        ParticipantQos make(Map<Class<? extends QosPolicy.ForDomainParticipant>, QosPolicy.ForDomainParticipant> p) {
            return new ParticipantQos(env, p);
        }

        @Override
        public UserData getUserData() {
            return get(UserData.class);
        }

        @Override
        public Property getProperty() {
            return get(Property.class);
        }

        /** An rdds_participant_qos. */
        MemorySegment toNative(Arena arena) {
            MemorySegment s = arena.allocate(32, 8);
            byte[] ud = ((Policies.UserDataImpl) getUserData()).value;
            s.set(JAVA_LONG, 0, ud.length == 0 ? 0 : arena.allocateFrom(JAVA_BYTE, ud).address());
            s.set(JAVA_LONG, 8, ud.length);
            // rdds_property: name and value, two pointers.
            Map<String, String> props = ((Policies.PropertyImpl) getProperty()).value;
            if (!props.isEmpty()) {
                MemorySegment a = arena.allocate(16L * props.size(), 8);
                long at = 0;
                for (Map.Entry<String, String> e : props.entrySet()) {
                    a.set(ADDRESS, at, arena.allocateFrom(e.getKey()));
                    a.set(ADDRESS, at + 8, arena.allocateFrom(e.getValue()));
                    at += 16;
                }
                s.set(JAVA_LONG, 16, a.address());
            }
            s.set(JAVA_LONG, 24, props.size());
            return s;
        }
    }

    static final class GroupQos<P extends QosPolicy> {
        private GroupQos() {}

        /** An rdds_group_qos of a PARTITION. */
        static MemorySegment toNative(Arena arena, Partition p) {
            MemorySegment s = arena.allocate(16, 8);
            List<String> names = new ArrayList<>(p.getName());
            if (!names.isEmpty()) {
                MemorySegment a = arena.allocate(ADDRESS, names.size());
                for (int i = 0; i < names.size(); i++) {
                    a.setAtIndex(ADDRESS, i, arena.allocateFrom(names.get(i)));
                }
                s.set(JAVA_LONG, 0, a.address());
            }
            s.set(JAVA_LONG, 8, names.size());
            return s;
        }
    }

    static final class PubQos extends Base<QosPolicy.ForPublisher, PubQos> implements PublisherQos {
        private static final long serialVersionUID = 1L;

        PubQos(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForPublisher>, QosPolicy.ForPublisher> p) {
            super(env, p);
        }

        static PubQos defaults(ServiceEnvironment env) {
            return new PubQos(env, map(env.getSPI().getPolicyFactory().Partition()));
        }

        @Override
        PubQos make(Map<Class<? extends QosPolicy.ForPublisher>, QosPolicy.ForPublisher> p) {
            return new PubQos(env, p);
        }

        @Override
        public Partition getPartition() {
            return get(Partition.class);
        }
    }

    static final class SubQos extends Base<QosPolicy.ForSubscriber, SubQos> implements SubscriberQos {
        private static final long serialVersionUID = 1L;

        SubQos(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForSubscriber>, QosPolicy.ForSubscriber> p) {
            super(env, p);
        }

        static SubQos defaults(ServiceEnvironment env) {
            return new SubQos(env, map(env.getSPI().getPolicyFactory().Partition()));
        }

        @Override
        SubQos make(Map<Class<? extends QosPolicy.ForSubscriber>, QosPolicy.ForSubscriber> p) {
            return new SubQos(env, p);
        }

        @Override
        public Partition getPartition() {
            return get(Partition.class);
        }
    }

    // The fields of rdds_writer_qos and rdds_reader_qos (rustorb/dds.h),
    // which share their first 88 bytes.
    static final long RELIABILITY_KIND = 0;
    static final long MAX_BLOCKING_TIME = 4;
    static final long HISTORY_KIND = 12;
    static final long HISTORY_DEPTH = 16;
    static final long MAX_SAMPLES = 20;
    static final long MAX_INSTANCES = 24;
    static final long MAX_SAMPLES_PER_INSTANCE = 28;
    static final long DURABILITY_KIND = 32;
    static final long DEADLINE_PERIOD = 36;
    static final long LIVELINESS_KIND = 44;
    static final long LEASE_DURATION = 48;
    static final long USER_DATA = 56;
    static final long USER_DATA_LEN = 64;
    static final long DATA_REPRESENTATION = 72;
    static final long DATA_REPRESENTATION_LEN = 80;
    static final long AUTODISPOSE = 88;
    static final long WRITER_QOS_SIZE = 96;
    static final long READER_QOS_SIZE = 104;

    /** The endpoint policies of an rdds_writer_qos / rdds_reader_qos. */
    static List<QosPolicy> fromNative(ServiceEnvironment env, MemorySegment s) {
        List<QosPolicy> l = new ArrayList<>();
        l.add(new Policies.ReliabilityImpl(env, Reliability.Kind.values()[s.get(JAVA_INT, RELIABILITY_KIND)],
                DurationImpl.get(s, MAX_BLOCKING_TIME)));
        l.add(new Policies.HistoryImpl(env, History.Kind.values()[s.get(JAVA_INT, HISTORY_KIND)],
                s.get(JAVA_INT, HISTORY_DEPTH)));
        l.add(new Policies.ResourceLimitsImpl(env, s.get(JAVA_INT, MAX_SAMPLES), s.get(JAVA_INT, MAX_INSTANCES),
                s.get(JAVA_INT, MAX_SAMPLES_PER_INSTANCE)));
        l.add(new Policies.DurabilityImpl(env, Durability.Kind.values()[s.get(JAVA_INT, DURABILITY_KIND)]));
        l.add(new Policies.DeadlineImpl(env, DurationImpl.get(s, DEADLINE_PERIOD)));
        l.add(new Policies.LivelinessImpl(env, Liveliness.Kind.values()[s.get(JAVA_INT, LIVELINESS_KIND)],
                DurationImpl.get(s, LEASE_DURATION)));
        long n = s.get(JAVA_LONG, USER_DATA_LEN);
        byte[] ud = new byte[(int) n];
        if (n > 0) {
            MemorySegment.copy(Library.MEMORY, JAVA_BYTE, s.get(JAVA_LONG, USER_DATA), ud, 0, ud.length);
        }
        l.add(new Policies.UserDataImpl(env, ud));
        long rn = s.get(JAVA_LONG, DATA_REPRESENTATION_LEN);
        List<Short> reps = new ArrayList<>();
        for (int i = 0; i < rn; i++) {
            reps.add(Library.MEMORY.get(JAVA_SHORT, s.get(JAVA_LONG, DATA_REPRESENTATION) + 2L * i));
        }
        l.add(new Policies.DataRepresentationImpl(env, reps));
        return l;
    }

    private static <T> T pol(Map<?, ?> q, Class<T> c) {
        return c.cast(q.get(c));
    }

    /** Writes the endpoint policies of q into s (an rdds_writer_qos or
     * rdds_reader_qos holding the defaults). */
    static void toNative(Arena arena, MemorySegment s, Map<?, ?> q) {
        Reliability r = pol(q, Reliability.class);
        s.set(JAVA_INT, RELIABILITY_KIND, r.getKind().ordinal());
        DurationImpl.put(s.asSlice(MAX_BLOCKING_TIME, 8), DurationImpl.nanos(r.getMaxBlockingTime()));
        History h = pol(q, History.class);
        s.set(JAVA_INT, HISTORY_KIND, h.getKind().ordinal());
        s.set(JAVA_INT, HISTORY_DEPTH, h.getDepth());
        ResourceLimits rl = pol(q, ResourceLimits.class);
        s.set(JAVA_INT, MAX_SAMPLES, rl.getMaxSamples());
        s.set(JAVA_INT, MAX_INSTANCES, rl.getMaxInstances());
        s.set(JAVA_INT, MAX_SAMPLES_PER_INSTANCE, rl.getMaxSamplesPerInstance());
        s.set(JAVA_INT, DURABILITY_KIND, pol(q, Durability.class).getKind().ordinal());
        DurationImpl.put(s.asSlice(DEADLINE_PERIOD, 8), DurationImpl.nanos(pol(q, Deadline.class).getPeriod()));
        Liveliness lv = pol(q, Liveliness.class);
        s.set(JAVA_INT, LIVELINESS_KIND, lv.getKind().ordinal());
        DurationImpl.put(s.asSlice(LEASE_DURATION, 8), DurationImpl.nanos(lv.getLeaseDuration()));
        UserData ud = pol(q, UserData.class);
        if (ud != null) {
            byte[] v = ud.getValue();
            s.set(JAVA_LONG, USER_DATA, v.length == 0 ? 0 : arena.allocateFrom(JAVA_BYTE, v).address());
            s.set(JAVA_LONG, USER_DATA_LEN, v.length);
        }
        List<Short> reps = pol(q, DataRepresentation.class).getValue();
        short[] rv = new short[reps.size()];
        for (int i = 0; i < rv.length; i++) {
            rv[i] = reps.get(i);
        }
        s.set(JAVA_LONG, DATA_REPRESENTATION, rv.length == 0 ? 0 : arena.allocateFrom(JAVA_SHORT, rv).address());
        s.set(JAVA_LONG, DATA_REPRESENTATION_LEN, rv.length);
    }

    static final class WriterQos extends Base<QosPolicy.ForDataWriter, WriterQos> implements DataWriterQos {
        private static final long serialVersionUID = 1L;

        WriterQos(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForDataWriter>, QosPolicy.ForDataWriter> p) {
            super(env, p);
        }

        /** The defaults of rdds_writer_qos_default. */
        static WriterQos defaults(ServiceEnvironment env) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment s = arena.allocate(WRITER_QOS_SIZE, 8);
                Errors.check(env, (int) Ffi.rdds_writer_qos_default.invokeExact(s.address()));
                Map<Class<? extends QosPolicy.ForDataWriter>, QosPolicy.ForDataWriter> m = new LinkedHashMap<>();
                for (QosPolicy p : fromNative(env, s)) {
                    put(m, (QosPolicy.ForDataWriter) p);
                }
                put(m, new Policies.WriterDataLifecycleImpl(env, s.get(JAVA_BOOLEAN, AUTODISPOSE)));
                return new WriterQos(env, m);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        @SuppressWarnings("unchecked")
        private static void put(Map<Class<? extends QosPolicy.ForDataWriter>, QosPolicy.ForDataWriter> m,
                QosPolicy.ForDataWriter p) {
            m.put((Class<? extends QosPolicy.ForDataWriter>) ((Policies.Base) p).id(), p);
        }

        /** This QoS with the policies of the topic QoS that writers have. */
        WriterQos withTopic(TopicQos t) {
            WriterQos q = this;
            for (QosPolicy.ForTopic p : t.values()) {
                if (p instanceof QosPolicy.ForDataWriter w) {
                    q = q.withPolicy(w);
                }
            }
            return q;
        }

        @Override
        WriterQos make(Map<Class<? extends QosPolicy.ForDataWriter>, QosPolicy.ForDataWriter> p) {
            return new WriterQos(env, p);
        }

        /** An rdds_writer_qos. */
        MemorySegment toNative(Arena arena) {
            MemorySegment s = arena.allocate(WRITER_QOS_SIZE, 8);
            try {
                Errors.check(env, (int) Ffi.rdds_writer_qos_default.invokeExact(s.address()));
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
            Qos.toNative(arena, s, this);
            s.set(JAVA_BOOLEAN, AUTODISPOSE, getWriterDataLifecycle().isAutDisposeUnregisteredInstances());
            return s;
        }

        @Override
        public Durability getDurability() {
            return get(Durability.class);
        }

        @Override
        public Deadline getDeadline() {
            return get(Deadline.class);
        }

        @Override
        public Liveliness getLiveliness() {
            return get(Liveliness.class);
        }

        @Override
        public Reliability getReliability() {
            return get(Reliability.class);
        }

        @Override
        public History getHistory() {
            return get(History.class);
        }

        @Override
        public ResourceLimits getResourceLimits() {
            return get(ResourceLimits.class);
        }

        @Override
        public UserData getUserData() {
            return get(UserData.class);
        }

        @Override
        public WriterDataLifecycle getWriterDataLifecycle() {
            return get(WriterDataLifecycle.class);
        }

        @Override
        public DataRepresentation getRepresentation() {
            return get(DataRepresentation.class);
        }
    }

    static final class ReaderQos extends Base<QosPolicy.ForDataReader, ReaderQos> implements DataReaderQos {
        private static final long serialVersionUID = 1L;

        ReaderQos(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForDataReader>, QosPolicy.ForDataReader> p) {
            super(env, p);
        }

        /** The defaults of rdds_reader_qos_default. */
        static ReaderQos defaults(ServiceEnvironment env) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment s = arena.allocate(READER_QOS_SIZE, 8);
                Errors.check(env, (int) Ffi.rdds_reader_qos_default.invokeExact(s.address()));
                Map<Class<? extends QosPolicy.ForDataReader>, QosPolicy.ForDataReader> m = new LinkedHashMap<>();
                for (QosPolicy p : fromNative(env, s)) {
                    @SuppressWarnings("unchecked")
                    Class<? extends QosPolicy.ForDataReader> id =
                            (Class<? extends QosPolicy.ForDataReader>) ((Policies.Base) p).id();
                    m.put(id, (QosPolicy.ForDataReader) p);
                }
                return new ReaderQos(env, m);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        ReaderQos withTopic(TopicQos t) {
            ReaderQos q = this;
            for (QosPolicy.ForTopic p : t.values()) {
                if (p instanceof QosPolicy.ForDataReader r) {
                    q = q.withPolicy(r);
                }
            }
            return q;
        }

        @Override
        ReaderQos make(Map<Class<? extends QosPolicy.ForDataReader>, QosPolicy.ForDataReader> p) {
            return new ReaderQos(env, p);
        }

        /** An rdds_reader_qos. */
        MemorySegment toNative(Arena arena) {
            MemorySegment s = arena.allocate(READER_QOS_SIZE, 8);
            try {
                Errors.check(env, (int) Ffi.rdds_reader_qos_default.invokeExact(s.address()));
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
            Qos.toNative(arena, s, this);
            return s;
        }

        @Override
        public Durability getDurability() {
            return get(Durability.class);
        }

        @Override
        public Deadline getDeadline() {
            return get(Deadline.class);
        }

        @Override
        public Liveliness getLiveliness() {
            return get(Liveliness.class);
        }

        @Override
        public Reliability getReliability() {
            return get(Reliability.class);
        }

        @Override
        public History getHistory() {
            return get(History.class);
        }

        @Override
        public ResourceLimits getResourceLimits() {
            return get(ResourceLimits.class);
        }

        @Override
        public UserData getUserData() {
            return get(UserData.class);
        }

        @Override
        public DataRepresentation getRepresentation() {
            return get(DataRepresentation.class);
        }
    }

    /** The topic QoS: the policies of writers and readers it has, with the
     * defaults of a reader (RELIABILITY: of a writer, as DDS 1.4 says for
     * topics). */
    static final class TopicQosImpl extends Base<QosPolicy.ForTopic, TopicQosImpl> implements TopicQos {
        private static final long serialVersionUID = 1L;

        TopicQosImpl(ServiceEnvironment env, Map<Class<? extends QosPolicy.ForTopic>, QosPolicy.ForTopic> p) {
            super(env, p);
        }

        static TopicQosImpl defaults(ServiceEnvironment env) {
            ReaderQos r = ReaderQos.defaults(env);
            WriterQos w = WriterQos.defaults(env);
            Map<Class<? extends QosPolicy.ForTopic>, QosPolicy.ForTopic> m = new LinkedHashMap<>();
            m.put(Durability.class, r.getDurability());
            m.put(Deadline.class, r.getDeadline());
            m.put(Liveliness.class, r.getLiveliness());
            m.put(Reliability.class, w.getReliability().withBestEffort());
            m.put(History.class, r.getHistory());
            m.put(ResourceLimits.class, r.getResourceLimits());
            m.put(DataRepresentation.class, r.getRepresentation());
            return new TopicQosImpl(env, m);
        }

        @Override
        TopicQosImpl make(Map<Class<? extends QosPolicy.ForTopic>, QosPolicy.ForTopic> p) {
            return new TopicQosImpl(env, p);
        }

        @Override
        public Durability getDurability() {
            return get(Durability.class);
        }

        @Override
        public Deadline getDeadline() {
            return get(Deadline.class);
        }

        @Override
        public Liveliness getLiveliness() {
            return get(Liveliness.class);
        }

        @Override
        public Reliability getReliability() {
            return get(Reliability.class);
        }

        @Override
        public History getHistory() {
            return get(History.class);
        }

        @Override
        public ResourceLimits getResourceLimits() {
            return get(ResourceLimits.class);
        }

        @Override
        public DataRepresentation getRepresentation() {
            return get(DataRepresentation.class);
        }
    }
}
