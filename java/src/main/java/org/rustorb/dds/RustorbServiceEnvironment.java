// The ServiceEnvironment of rustorb-dds: ServiceEnvironment.createInstance
// picks it when org.omg.dds.serviceClassName is not set.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.util.Map;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import org.omg.dds.core.Duration;
import org.omg.dds.core.GuardCondition;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.ModifiableTime;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.Time;
import org.omg.dds.core.WaitSet;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.status.Status;
import org.omg.dds.domain.DomainParticipantFactory;
import org.omg.dds.type.TypeSupport;

/** The rustorb-dds implementation of the DDS Java 5 PSM. */
public final class RustorbServiceEnvironment extends ServiceEnvironment {
    private final Spi spi = new Spi();

    public RustorbServiceEnvironment() {
    }

    public RustorbServiceEnvironment(Map<String, Object> environment) {
        this();
    }

    @Override
    public ServiceProviderInterface getSPI() {
        return spi;
    }

    private final class Spi implements ServiceProviderInterface {
        private final ServiceEnvironment env = RustorbServiceEnvironment.this;
        private volatile ParticipantImpl.Factory factory;
        private volatile Policies.Factory policies;

        @Override
        public DomainParticipantFactory getParticipantFactory() {
            ParticipantImpl.Factory f = factory;
            if (f == null) {
                synchronized (this) {
                    f = factory;
                    if (f == null) {
                        f = new ParticipantImpl.Factory(env);
                        factory = f;
                    }
                }
            }
            return f;
        }

        @Override
        public <TYPE> TypeSupport<TYPE> newTypeSupport(Class<TYPE> type, String registeredName) {
            return new TypeSupportImpl<>(env, TypeSupportImpl.codecOf(type), registeredName);
        }

        @Override
        public Duration newDuration(long duration, TimeUnit unit) {
            return DurationImpl.of(env, duration, unit);
        }

        @Override
        public Duration infiniteDuration() {
            return new DurationImpl(env, DurationImpl.INFINITE);
        }

        @Override
        public Duration zeroDuration() {
            return new DurationImpl(env, 0);
        }

        @Override
        public ModifiableTime newTime(long time, TimeUnit units) {
            ModifiableTime t = new TimeImpl(env, 0);
            t.setTime(time, units);
            return t;
        }

        @Override
        public Time invalidTime() {
            return new TimeImpl(env, TimeImpl.INVALID);
        }

        @Override
        public ModifiableInstanceHandle newInstanceHandle() {
            return HandleImpl.nil(env);
        }

        @Override
        public InstanceHandle nilHandle() {
            return HandleImpl.nil(env);
        }

        @Override
        public GuardCondition newGuardCondition() {
            return new Conditions.Guard(env);
        }

        @Override
        public WaitSet newWaitSet() {
            return new Conditions.WaitSetImpl(env);
        }

        @Override
        public Set<Class<? extends Status>> allStatusKinds() {
            return StatusKinds.all();
        }

        @Override
        public Set<Class<? extends Status>> noStatusKinds() {
            return Set.of();
        }

        @Override
        public PolicyFactory getPolicyFactory() {
            Policies.Factory p = policies;
            if (p == null) {
                p = new Policies.Factory(env);
                policies = p;
            }
            return p;
        }
    }
}
