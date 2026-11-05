// The QoS policies of the PSM: immutable values.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.TimeUnit;

import org.omg.dds.core.Duration;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.policy.DataRepresentation;
import org.omg.dds.core.policy.Deadline;
import org.omg.dds.core.policy.Durability;
import org.omg.dds.core.policy.History;
import org.omg.dds.core.policy.Liveliness;
import org.omg.dds.core.policy.Partition;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.Reliability;
import org.omg.dds.core.policy.ResourceLimits;
import org.omg.dds.core.policy.UserData;
import org.omg.dds.core.policy.WriterDataLifecycle;

final class Policies {
    private Policies() {}

    /** A policy of this implementation: its id is its PSM interface. */
    abstract static class Base implements QosPolicy {
        private static final long serialVersionUID = 1L;
        final transient ServiceEnvironment env;

        Base(ServiceEnvironment env) {
            this.env = env;
        }

        abstract Class<? extends QosPolicy> id();

        abstract List<Object> state();

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public boolean equals(Object o) {
            return o instanceof Base b && b.id() == id() && b.state().equals(state());
        }

        @Override
        public int hashCode() {
            return Objects.hash(id(), state());
        }

        @Override
        public String toString() {
            return id().getSimpleName() + state();
        }
    }

    static Duration duration(ServiceEnvironment env, long nanos) {
        return new DurationImpl(env, nanos);
    }

    static final class ReliabilityImpl extends Base implements Reliability {
        private static final long serialVersionUID = 1L;
        final Kind kind;
        final long maxBlocking;

        ReliabilityImpl(ServiceEnvironment env, Kind kind, long maxBlocking) {
            super(env);
            this.kind = Objects.requireNonNull(kind);
            this.maxBlocking = maxBlocking;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return Reliability.class;
        }

        @Override
        List<Object> state() {
            return List.of(kind, maxBlocking);
        }

        @Override
        public Kind getKind() {
            return kind;
        }

        @Override
        public Duration getMaxBlockingTime() {
            return duration(env, maxBlocking);
        }

        @Override
        public Reliability withKind(Kind k) {
            return new ReliabilityImpl(env, k, maxBlocking);
        }

        @Override
        public Reliability withMaxBlockingTime(Duration d) {
            return new ReliabilityImpl(env, kind, DurationImpl.nanos(d));
        }

        @Override
        public Reliability withMaxBlockingTime(long d, TimeUnit unit) {
            return new ReliabilityImpl(env, kind, DurationImpl.of(env, d, unit).nanos);
        }

        @Override
        public Reliability withBestEffort() {
            return withKind(Kind.BEST_EFFORT);
        }

        @Override
        public Reliability withReliable() {
            return withKind(Kind.RELIABLE);
        }
    }

    static final class HistoryImpl extends Base implements History {
        private static final long serialVersionUID = 1L;
        final Kind kind;
        final int depth;

        HistoryImpl(ServiceEnvironment env, Kind kind, int depth) {
            super(env);
            this.kind = Objects.requireNonNull(kind);
            this.depth = depth;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return History.class;
        }

        @Override
        List<Object> state() {
            return List.of(kind, depth);
        }

        @Override
        public Kind getKind() {
            return kind;
        }

        @Override
        public int getDepth() {
            return depth;
        }

        @Override
        public History withKind(Kind k) {
            return new HistoryImpl(env, k, depth);
        }

        @Override
        public History withDepth(int d) {
            return new HistoryImpl(env, kind, d);
        }

        @Override
        public History withKeepAll() {
            return withKind(Kind.KEEP_ALL);
        }

        @Override
        public History withKeepLast(int d) {
            return new HistoryImpl(env, Kind.KEEP_LAST, d);
        }
    }

    static final class DurabilityImpl extends Base implements Durability {
        private static final long serialVersionUID = 1L;
        final Kind kind;

        DurabilityImpl(ServiceEnvironment env, Kind kind) {
            super(env);
            this.kind = Objects.requireNonNull(kind);
        }

        @Override
        Class<? extends QosPolicy> id() {
            return Durability.class;
        }

        @Override
        List<Object> state() {
            return List.of(kind);
        }

        @Override
        public Kind getKind() {
            return kind;
        }

        @Override
        public Durability withKind(Kind k) {
            return new DurabilityImpl(env, k);
        }

        @Override
        public Durability withVolatile() {
            return withKind(Kind.VOLATILE);
        }

        @Override
        public Durability withTransientLocal() {
            return withKind(Kind.TRANSIENT_LOCAL);
        }

        @Override
        public Durability withTransient() {
            return withKind(Kind.TRANSIENT);
        }

        @Override
        public Durability withPersitent() {
            return withKind(Kind.PERSISTENT);
        }
    }

    static final class DeadlineImpl extends Base implements Deadline {
        private static final long serialVersionUID = 1L;
        final long period;

        DeadlineImpl(ServiceEnvironment env, long period) {
            super(env);
            this.period = period;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return Deadline.class;
        }

        @Override
        List<Object> state() {
            return List.of(period);
        }

        @Override
        public Duration getPeriod() {
            return duration(env, period);
        }

        @Override
        public Deadline withPeriod(Duration p) {
            return new DeadlineImpl(env, DurationImpl.nanos(p));
        }

        @Override
        public Deadline withPeriod(long p, TimeUnit unit) {
            return new DeadlineImpl(env, DurationImpl.of(env, p, unit).nanos);
        }
    }

    static final class LivelinessImpl extends Base implements Liveliness {
        private static final long serialVersionUID = 1L;
        final Kind kind;
        final long lease;

        LivelinessImpl(ServiceEnvironment env, Kind kind, long lease) {
            super(env);
            this.kind = Objects.requireNonNull(kind);
            this.lease = lease;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return Liveliness.class;
        }

        @Override
        List<Object> state() {
            return List.of(kind, lease);
        }

        @Override
        public Kind getKind() {
            return kind;
        }

        @Override
        public Duration getLeaseDuration() {
            return duration(env, lease);
        }

        @Override
        public Liveliness withKind(Kind k) {
            return new LivelinessImpl(env, k, lease);
        }

        @Override
        public Liveliness withLeaseDuration(Duration d) {
            return new LivelinessImpl(env, kind, DurationImpl.nanos(d));
        }

        @Override
        public Liveliness withLeaseDuration(long d, TimeUnit unit) {
            return new LivelinessImpl(env, kind, DurationImpl.of(env, d, unit).nanos);
        }

        @Override
        public Liveliness withAutomatic() {
            return withKind(Kind.AUTOMATIC);
        }

        @Override
        public Liveliness withManualByParticipant() {
            return withKind(Kind.MANUAL_BY_PARTICIPANT);
        }

        @Override
        public Liveliness withManualByTopic() {
            return withKind(Kind.MANUAL_BY_TOPIC);
        }
    }

    static final class ResourceLimitsImpl extends Base implements ResourceLimits {
        private static final long serialVersionUID = 1L;
        final int maxSamples;
        final int maxInstances;
        final int maxSamplesPerInstance;

        ResourceLimitsImpl(ServiceEnvironment env, int maxSamples, int maxInstances, int maxSamplesPerInstance) {
            super(env);
            this.maxSamples = maxSamples;
            this.maxInstances = maxInstances;
            this.maxSamplesPerInstance = maxSamplesPerInstance;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return ResourceLimits.class;
        }

        @Override
        List<Object> state() {
            return List.of(maxSamples, maxInstances, maxSamplesPerInstance);
        }

        @Override
        public int getMaxSamples() {
            return maxSamples;
        }

        @Override
        public int getMaxInstances() {
            return maxInstances;
        }

        @Override
        public int getMaxSamplesPerInstance() {
            return maxSamplesPerInstance;
        }

        @Override
        public ResourceLimits withMaxSamples(int n) {
            return new ResourceLimitsImpl(env, n, maxInstances, maxSamplesPerInstance);
        }

        @Override
        public ResourceLimits withMaxInstances(int n) {
            return new ResourceLimitsImpl(env, maxSamples, n, maxSamplesPerInstance);
        }

        @Override
        public ResourceLimits withMaxSamplesPerInstance(int n) {
            return new ResourceLimitsImpl(env, maxSamples, maxInstances, n);
        }
    }

    static final class PartitionImpl extends Base implements Partition {
        private static final long serialVersionUID = 1L;
        final Set<String> names;

        PartitionImpl(ServiceEnvironment env, Collection<String> names) {
            super(env);
            this.names = Collections.unmodifiableSet(new LinkedHashSet<>(names));
        }

        @Override
        Class<? extends QosPolicy> id() {
            return Partition.class;
        }

        @Override
        List<Object> state() {
            return List.of(new ArrayList<>(names));
        }

        @Override
        public Set<String> getName() {
            return names;
        }

        @Override
        public Partition withName(Collection<String> name) {
            return new PartitionImpl(env, name);
        }
    }

    static final class UserDataImpl extends Base implements UserData {
        private static final long serialVersionUID = 1L;
        final byte[] value;

        UserDataImpl(ServiceEnvironment env, byte[] value) {
            super(env);
            this.value = value;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return UserData.class;
        }

        @Override
        List<Object> state() {
            return List.of(Arrays.toString(value));
        }

        @Override
        public byte[] getValue() {
            return value.clone();
        }

        @Override
        public UserData withValue(byte[] v, int offset, int length) {
            return new UserDataImpl(env, Arrays.copyOfRange(v, offset, offset + length));
        }
    }

    static final class WriterDataLifecycleImpl extends Base implements WriterDataLifecycle {
        private static final long serialVersionUID = 1L;
        final boolean autodispose;

        WriterDataLifecycleImpl(ServiceEnvironment env, boolean autodispose) {
            super(env);
            this.autodispose = autodispose;
        }

        @Override
        Class<? extends QosPolicy> id() {
            return WriterDataLifecycle.class;
        }

        @Override
        List<Object> state() {
            return List.of(autodispose);
        }

        @Override
        public boolean isAutDisposeUnregisteredInstances() {
            return autodispose;
        }

        @Override
        public WriterDataLifecycle withAutDisposeUnregisteredInstances(boolean v) {
            return new WriterDataLifecycleImpl(env, v);
        }
    }

    static final class DataRepresentationImpl extends Base implements DataRepresentation {
        private static final long serialVersionUID = 1L;
        final List<Short> value;

        DataRepresentationImpl(ServiceEnvironment env, List<Short> value) {
            super(env);
            this.value = List.copyOf(value);
        }

        @Override
        Class<? extends QosPolicy> id() {
            return DataRepresentation.class;
        }

        @Override
        List<Object> state() {
            return List.of(value);
        }

        @Override
        public List<Short> getValue() {
            return value;
        }

        @Override
        public DataRepresentation withValue(List<Short> v) {
            return new DataRepresentationImpl(env, v);
        }

        @Override
        public DataRepresentation withValue(short... v) {
            List<Short> l = new ArrayList<>();
            for (short s : v) {
                l.add(s);
            }
            return new DataRepresentationImpl(env, l);
        }
    }

    /** The policies with the defaults of a DataReader (but RELIABILITY,
     * whose default is BEST_EFFORT for readers and RELIABLE for writers). */
    static final class Factory extends PolicyFactory {
        private final ServiceEnvironment env;

        Factory(ServiceEnvironment env) {
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public Durability Durability() {
            return new DurabilityImpl(env, Durability.Kind.VOLATILE);
        }

        @Override
        public Deadline Deadline() {
            return new DeadlineImpl(env, DurationImpl.INFINITE);
        }

        @Override
        public Liveliness Liveliness() {
            return new LivelinessImpl(env, Liveliness.Kind.AUTOMATIC, DurationImpl.INFINITE);
        }

        @Override
        public History History() {
            return new HistoryImpl(env, History.Kind.KEEP_LAST, 1);
        }

        @Override
        public ResourceLimits ResourceLimits() {
            return new ResourceLimitsImpl(env, ResourceLimits.LENGTH_UNLIMITED, ResourceLimits.LENGTH_UNLIMITED,
                    ResourceLimits.LENGTH_UNLIMITED);
        }

        @Override
        public UserData UserData() {
            return new UserDataImpl(env, new byte[0]);
        }

        @Override
        public DataRepresentation Representation() {
            return new DataRepresentationImpl(env, List.of());
        }

        @Override
        public Reliability Reliability() {
            return new ReliabilityImpl(env, Reliability.Kind.BEST_EFFORT, 100_000_000L);
        }

        @Override
        public WriterDataLifecycle WriterDataLifecycle() {
            return new WriterDataLifecycleImpl(env, true);
        }

        @Override
        public Partition Partition() {
            return new PartitionImpl(env, List.of());
        }
    }
}
