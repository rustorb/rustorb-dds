// The statuses of the PSM, read from the C ABI's status structs, and the
// events listeners get.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;

import java.lang.foreign.MemorySegment;
import java.util.Collections;
import java.util.LinkedHashSet;
import java.util.Set;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.event.*;
import org.omg.dds.core.policy.DataRepresentation;
import org.omg.dds.core.policy.Deadline;
import org.omg.dds.core.policy.Durability;
import org.omg.dds.core.policy.History;
import org.omg.dds.core.policy.Liveliness;
import org.omg.dds.core.policy.Partition;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.QosPolicyCount;
import org.omg.dds.core.policy.Reliability;
import org.omg.dds.core.policy.ResourceLimits;
import org.omg.dds.core.policy.UserData;
import org.omg.dds.core.policy.WriterDataLifecycle;
import org.omg.dds.core.status.*;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.topic.Topic;

final class Statuses {
    private Statuses() {}

    // The sizes of the status structs of rustorb/dds.h.
    static final long COUNT_SIZE = 8;
    static final long DEADLINE_SIZE = 24;
    static final long MATCHED_SIZE = 32;
    static final long LIVELINESS_SIZE = 32;
    static final long INCOMPATIBLE_SIZE = 272;
    static final long REJECTED_SIZE = 28;

    /** The PSM class of a QosPolicyId_t (DDS 1.4, 2.2.3); QosPolicy for
     * the policies the PSM subset has no class for. */
    static Class<? extends QosPolicy> policyClass(int id) {
        switch (id) {
            case 1:
                return UserData.class;
            case 2:
                return Durability.class;
            case 4:
                return Deadline.class;
            case 8:
                return Liveliness.class;
            case 10:
                return Partition.class;
            case 11:
                return Reliability.class;
            case 13:
                return History.class;
            case 14:
                return ResourceLimits.class;
            case 16:
                return WriterDataLifecycle.class;
            case 23:
                return DataRepresentation.class;
            default:
                return QosPolicy.class;
        }
    }

    static Set<QosPolicyCount> policyCounts(ServiceEnvironment env, MemorySegment s) {
        int n = Math.min(s.get(JAVA_INT, 12), 32);
        Set<QosPolicyCount> set = new LinkedHashSet<>();
        for (int i = 0; i < n; i++) {
            set.add(new PolicyCount(env, policyClass(s.get(JAVA_INT, 16 + 8L * i)), s.get(JAVA_INT, 20 + 8L * i)));
        }
        return Collections.unmodifiableSet(set);
    }

    record PolicyCount(ServiceEnvironment env, Class<? extends QosPolicy> policy, int count)
            implements QosPolicyCount {
        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public Class<? extends QosPolicy> getPolicyClass() {
            return policy;
        }

        @Override
        public int getCount() {
            return count;
        }
    }

    static final class InconsistentTopic extends InconsistentTopicStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;

        InconsistentTopic(ServiceEnvironment env, int totalCount, int totalCountChange) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
        }

        /** The status in the C struct at s. */
        static InconsistentTopic of(ServiceEnvironment env, MemorySegment s) {
            return new InconsistentTopic(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public String toString() {
            return "InconsistentTopicStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ")";
        }
    }

    static final class LivelinessLost extends LivelinessLostStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;

        LivelinessLost(ServiceEnvironment env, int totalCount, int totalCountChange) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
        }

        /** The status in the C struct at s. */
        static LivelinessLost of(ServiceEnvironment env, MemorySegment s) {
            return new LivelinessLost(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public String toString() {
            return "LivelinessLostStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ")";
        }
    }

    static final class SampleLost extends SampleLostStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;

        SampleLost(ServiceEnvironment env, int totalCount, int totalCountChange) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
        }

        /** The status in the C struct at s. */
        static SampleLost of(ServiceEnvironment env, MemorySegment s) {
            return new SampleLost(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public String toString() {
            return "SampleLostStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ")";
        }
    }

    static final class OfferedDeadlineMissed extends OfferedDeadlineMissedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final InstanceHandle lastInstanceHandle;

        OfferedDeadlineMissed(ServiceEnvironment env, int totalCount, int totalCountChange, InstanceHandle lastInstanceHandle) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.lastInstanceHandle = lastInstanceHandle;
        }

        /** The status in the C struct at s. */
        static OfferedDeadlineMissed of(ServiceEnvironment env, MemorySegment s) {
            return new OfferedDeadlineMissed(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), HandleImpl.get(env, s, 8));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public InstanceHandle getLastInstanceHandle() {
            return lastInstanceHandle;
        }

        @Override
        public String toString() {
            return "OfferedDeadlineMissedStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "LastInstanceHandle=" + lastInstanceHandle + ")";
        }
    }

    static final class RequestedDeadlineMissed extends RequestedDeadlineMissedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final InstanceHandle lastInstanceHandle;

        RequestedDeadlineMissed(ServiceEnvironment env, int totalCount, int totalCountChange, InstanceHandle lastInstanceHandle) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.lastInstanceHandle = lastInstanceHandle;
        }

        /** The status in the C struct at s. */
        static RequestedDeadlineMissed of(ServiceEnvironment env, MemorySegment s) {
            return new RequestedDeadlineMissed(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), HandleImpl.get(env, s, 8));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public InstanceHandle getLastInstanceHandle() {
            return lastInstanceHandle;
        }

        @Override
        public String toString() {
            return "RequestedDeadlineMissedStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "LastInstanceHandle=" + lastInstanceHandle + ")";
        }
    }

    static final class PublicationMatched extends PublicationMatchedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final int currentCount;
        private final int currentCountChange;
        private final InstanceHandle lastSubscriptionHandle;

        PublicationMatched(ServiceEnvironment env, int totalCount, int totalCountChange, int currentCount, int currentCountChange, InstanceHandle lastSubscriptionHandle) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.currentCount = currentCount;
            this.currentCountChange = currentCountChange;
            this.lastSubscriptionHandle = lastSubscriptionHandle;
        }

        /** The status in the C struct at s. */
        static PublicationMatched of(ServiceEnvironment env, MemorySegment s) {
            return new PublicationMatched(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), s.get(JAVA_INT, 8), s.get(JAVA_INT, 12), HandleImpl.get(env, s, 16));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public int getCurrentCount() {
            return currentCount;
        }

        @Override
        public int getCurrentCountChange() {
            return currentCountChange;
        }

        @Override
        public InstanceHandle getLastSubscriptionHandle() {
            return lastSubscriptionHandle;
        }

        @Override
        public String toString() {
            return "PublicationMatchedStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "CurrentCount=" + currentCount + ", " + "CurrentCountChange=" + currentCountChange + ", " + "LastSubscriptionHandle=" + lastSubscriptionHandle + ")";
        }
    }

    static final class SubscriptionMatched extends SubscriptionMatchedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final int currentCount;
        private final int currentCountChange;
        private final InstanceHandle lastPublicationHandle;

        SubscriptionMatched(ServiceEnvironment env, int totalCount, int totalCountChange, int currentCount, int currentCountChange, InstanceHandle lastPublicationHandle) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.currentCount = currentCount;
            this.currentCountChange = currentCountChange;
            this.lastPublicationHandle = lastPublicationHandle;
        }

        /** The status in the C struct at s. */
        static SubscriptionMatched of(ServiceEnvironment env, MemorySegment s) {
            return new SubscriptionMatched(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), s.get(JAVA_INT, 8), s.get(JAVA_INT, 12), HandleImpl.get(env, s, 16));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public int getCurrentCount() {
            return currentCount;
        }

        @Override
        public int getCurrentCountChange() {
            return currentCountChange;
        }

        @Override
        public InstanceHandle getLastPublicationHandle() {
            return lastPublicationHandle;
        }

        @Override
        public String toString() {
            return "SubscriptionMatchedStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "CurrentCount=" + currentCount + ", " + "CurrentCountChange=" + currentCountChange + ", " + "LastPublicationHandle=" + lastPublicationHandle + ")";
        }
    }

    static final class LivelinessChanged extends LivelinessChangedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int aliveCount;
        private final int notAliveCount;
        private final int aliveCountChange;
        private final int notAliveCountChange;
        private final InstanceHandle lastPublicationHandle;

        LivelinessChanged(ServiceEnvironment env, int aliveCount, int notAliveCount, int aliveCountChange, int notAliveCountChange, InstanceHandle lastPublicationHandle) {
            this.env = env;
            this.aliveCount = aliveCount;
            this.notAliveCount = notAliveCount;
            this.aliveCountChange = aliveCountChange;
            this.notAliveCountChange = notAliveCountChange;
            this.lastPublicationHandle = lastPublicationHandle;
        }

        /** The status in the C struct at s. */
        static LivelinessChanged of(ServiceEnvironment env, MemorySegment s) {
            return new LivelinessChanged(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), s.get(JAVA_INT, 8), s.get(JAVA_INT, 12), HandleImpl.get(env, s, 16));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getAliveCount() {
            return aliveCount;
        }

        @Override
        public int getNotAliveCount() {
            return notAliveCount;
        }

        @Override
        public int getAliveCountChange() {
            return aliveCountChange;
        }

        @Override
        public int getNotAliveCountChange() {
            return notAliveCountChange;
        }

        @Override
        public InstanceHandle getLastPublicationHandle() {
            return lastPublicationHandle;
        }

        @Override
        public String toString() {
            return "LivelinessChangedStatus" + "(" + "AliveCount=" + aliveCount + ", " + "NotAliveCount=" + notAliveCount + ", " + "AliveCountChange=" + aliveCountChange + ", " + "NotAliveCountChange=" + notAliveCountChange + ", " + "LastPublicationHandle=" + lastPublicationHandle + ")";
        }
    }

    static final class OfferedIncompatibleQos extends OfferedIncompatibleQosStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final Class<? extends QosPolicy> lastPolicyClass;
        private final Set<QosPolicyCount> policies;

        OfferedIncompatibleQos(ServiceEnvironment env, int totalCount, int totalCountChange, Class<? extends QosPolicy> lastPolicyClass, Set<QosPolicyCount> policies) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.lastPolicyClass = lastPolicyClass;
            this.policies = policies;
        }

        /** The status in the C struct at s. */
        static OfferedIncompatibleQos of(ServiceEnvironment env, MemorySegment s) {
            return new OfferedIncompatibleQos(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), policyClass(s.get(JAVA_INT, 8)), policyCounts(env, s));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public Class<? extends QosPolicy> getLastPolicyClass() {
            return lastPolicyClass;
        }

        @Override
        public Set<QosPolicyCount> getPolicies() {
            return policies;
        }

        @Override
        public String toString() {
            return "OfferedIncompatibleQosStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "LastPolicyClass=" + lastPolicyClass + ", " + "Policies=" + policies + ")";
        }
    }

    static final class RequestedIncompatibleQos extends RequestedIncompatibleQosStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final Class<? extends QosPolicy> lastPolicyClass;
        private final Set<QosPolicyCount> policies;

        RequestedIncompatibleQos(ServiceEnvironment env, int totalCount, int totalCountChange, Class<? extends QosPolicy> lastPolicyClass, Set<QosPolicyCount> policies) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.lastPolicyClass = lastPolicyClass;
            this.policies = policies;
        }

        /** The status in the C struct at s. */
        static RequestedIncompatibleQos of(ServiceEnvironment env, MemorySegment s) {
            return new RequestedIncompatibleQos(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), policyClass(s.get(JAVA_INT, 8)), policyCounts(env, s));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public Class<? extends QosPolicy> getLastPolicyClass() {
            return lastPolicyClass;
        }

        @Override
        public Set<QosPolicyCount> getPolicies() {
            return policies;
        }

        @Override
        public String toString() {
            return "RequestedIncompatibleQosStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "LastPolicyClass=" + lastPolicyClass + ", " + "Policies=" + policies + ")";
        }
    }

    static final class SampleRejected extends SampleRejectedStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final int totalCount;
        private final int totalCountChange;
        private final Kind lastReason;
        private final InstanceHandle lastInstanceHandle;

        SampleRejected(ServiceEnvironment env, int totalCount, int totalCountChange, Kind lastReason, InstanceHandle lastInstanceHandle) {
            this.env = env;
            this.totalCount = totalCount;
            this.totalCountChange = totalCountChange;
            this.lastReason = lastReason;
            this.lastInstanceHandle = lastInstanceHandle;
        }

        /** The status in the C struct at s. */
        static SampleRejected of(ServiceEnvironment env, MemorySegment s) {
            return new SampleRejected(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4), SampleRejectedStatus.Kind.values()[s.get(JAVA_INT, 8)], HandleImpl.get(env, s, 12));
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public int getTotalCount() {
            return totalCount;
        }

        @Override
        public int getTotalCountChange() {
            return totalCountChange;
        }

        @Override
        public Kind getLastReason() {
            return lastReason;
        }

        @Override
        public InstanceHandle getLastInstanceHandle() {
            return lastInstanceHandle;
        }

        @Override
        public String toString() {
            return "SampleRejectedStatus" + "(" + "TotalCount=" + totalCount + ", " + "TotalCountChange=" + totalCountChange + ", " + "LastReason=" + lastReason + ", " + "LastInstanceHandle=" + lastInstanceHandle + ")";
        }
    }

    static final class DataAvailable extends DataAvailableStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        DataAvailable(ServiceEnvironment env) {
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public String toString() {
            return "DataAvailableStatus";
        }
    }

    static final class InconsistentTopicEv<T> extends InconsistentTopicEvent<T> {
        private static final long serialVersionUID = 1L;
        private final InconsistentTopicStatus status;

        InconsistentTopicEv(Topic<T> source, InconsistentTopicStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public Topic<T> getSource() {
            return (Topic<T>) source;
        }

        @Override
        public InconsistentTopicStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public InconsistentTopicEv<T> clone() {
            return new InconsistentTopicEv<>(getSource(), status);
        }
    }

    static final class OfferedDeadlineMissedEv<T> extends OfferedDeadlineMissedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final OfferedDeadlineMissedStatus status;

        OfferedDeadlineMissedEv(DataWriter<T> source, OfferedDeadlineMissedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataWriter<T> getSource() {
            return (DataWriter<T>) source;
        }

        @Override
        public OfferedDeadlineMissedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public OfferedDeadlineMissedEv<T> clone() {
            return new OfferedDeadlineMissedEv<>(getSource(), status);
        }
    }

    static final class OfferedIncompatibleQosEv<T> extends OfferedIncompatibleQosEvent<T> {
        private static final long serialVersionUID = 1L;
        private final OfferedIncompatibleQosStatus status;

        OfferedIncompatibleQosEv(DataWriter<T> source, OfferedIncompatibleQosStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataWriter<T> getSource() {
            return (DataWriter<T>) source;
        }

        @Override
        public OfferedIncompatibleQosStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public OfferedIncompatibleQosEv<T> clone() {
            return new OfferedIncompatibleQosEv<>(getSource(), status);
        }
    }

    static final class LivelinessLostEv<T> extends LivelinessLostEvent<T> {
        private static final long serialVersionUID = 1L;
        private final LivelinessLostStatus status;

        LivelinessLostEv(DataWriter<T> source, LivelinessLostStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataWriter<T> getSource() {
            return (DataWriter<T>) source;
        }

        @Override
        public LivelinessLostStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public LivelinessLostEv<T> clone() {
            return new LivelinessLostEv<>(getSource(), status);
        }
    }

    static final class PublicationMatchedEv<T> extends PublicationMatchedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final PublicationMatchedStatus status;

        PublicationMatchedEv(DataWriter<T> source, PublicationMatchedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataWriter<T> getSource() {
            return (DataWriter<T>) source;
        }

        @Override
        public PublicationMatchedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public PublicationMatchedEv<T> clone() {
            return new PublicationMatchedEv<>(getSource(), status);
        }
    }

    static final class RequestedDeadlineMissedEv<T> extends RequestedDeadlineMissedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final RequestedDeadlineMissedStatus status;

        RequestedDeadlineMissedEv(DataReader<T> source, RequestedDeadlineMissedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public RequestedDeadlineMissedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public RequestedDeadlineMissedEv<T> clone() {
            return new RequestedDeadlineMissedEv<>(getSource(), status);
        }
    }

    static final class RequestedIncompatibleQosEv<T> extends RequestedIncompatibleQosEvent<T> {
        private static final long serialVersionUID = 1L;
        private final RequestedIncompatibleQosStatus status;

        RequestedIncompatibleQosEv(DataReader<T> source, RequestedIncompatibleQosStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public RequestedIncompatibleQosStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public RequestedIncompatibleQosEv<T> clone() {
            return new RequestedIncompatibleQosEv<>(getSource(), status);
        }
    }

    static final class SampleRejectedEv<T> extends SampleRejectedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final SampleRejectedStatus status;

        SampleRejectedEv(DataReader<T> source, SampleRejectedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public SampleRejectedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public SampleRejectedEv<T> clone() {
            return new SampleRejectedEv<>(getSource(), status);
        }
    }

    static final class LivelinessChangedEv<T> extends LivelinessChangedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final LivelinessChangedStatus status;

        LivelinessChangedEv(DataReader<T> source, LivelinessChangedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public LivelinessChangedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public LivelinessChangedEv<T> clone() {
            return new LivelinessChangedEv<>(getSource(), status);
        }
    }

    static final class DataAvailableEv<T> extends DataAvailableEvent<T> {
        private static final long serialVersionUID = 1L;
        private final DataAvailableStatus status;

        DataAvailableEv(DataReader<T> source, DataAvailableStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public DataAvailableStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public DataAvailableEv<T> clone() {
            return new DataAvailableEv<>(getSource(), status);
        }
    }

    static final class SubscriptionMatchedEv<T> extends SubscriptionMatchedEvent<T> {
        private static final long serialVersionUID = 1L;
        private final SubscriptionMatchedStatus status;

        SubscriptionMatchedEv(DataReader<T> source, SubscriptionMatchedStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public SubscriptionMatchedStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public SubscriptionMatchedEv<T> clone() {
            return new SubscriptionMatchedEv<>(getSource(), status);
        }
    }

    static final class SampleLostEv<T> extends SampleLostEvent<T> {
        private static final long serialVersionUID = 1L;
        private final SampleLostStatus status;

        SampleLostEv(DataReader<T> source, SampleLostStatus status) {
            super(source);
            this.status = status;
        }

        @SuppressWarnings("unchecked")
        @Override
        public DataReader<T> getSource() {
            return (DataReader<T>) source;
        }

        @Override
        public SampleLostStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public SampleLostEv<T> clone() {
            return new SampleLostEv<>(getSource(), status);
        }
    }

    static final class DataOnReaders extends DataOnReadersStatus {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;

        DataOnReaders(ServiceEnvironment env) {
            this.env = env;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public String toString() {
            return "DataOnReadersStatus";
        }
    }

    static final class DataOnReadersEv extends DataOnReadersEvent {
        private static final long serialVersionUID = 1L;
        private final DataOnReadersStatus status;

        DataOnReadersEv(Subscriber source, DataOnReadersStatus status) {
            super(source);
            this.status = status;
        }

        @Override
        public Subscriber getSource() {
            return (Subscriber) source;
        }

        @Override
        public DataOnReadersStatus getStatus() {
            return status;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return getSource().getEnvironment();
        }

        @Override
        public DataOnReadersEv clone() {
            return new DataOnReadersEv(getSource(), status);
        }
    }
}
