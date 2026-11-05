// The conditions and the WaitSet of the PSM, on rdds_condition and
// rdds_waitset.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.ref.Cleaner;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import org.omg.dds.core.Condition;
import org.omg.dds.core.Duration;
import org.omg.dds.core.Entity;
import org.omg.dds.core.GuardCondition;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.WaitSet;
import org.omg.dds.core.status.Status;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.InstanceState;
import org.omg.dds.sub.ReadCondition;
import org.omg.dds.sub.SampleState;
import org.omg.dds.sub.ViewState;

final class Conditions {
    private Conditions() {}

    static final Cleaner CLEANER = Cleaner.create();

    /** A condition with an rdds_condition, released when it is closed or
     * unreachable. */
    abstract static class Native implements Condition {
        final ServiceEnvironment env;
        private final long handle;
        private final Cleaner.Cleanable cleanable;
        private volatile boolean closed;

        Native(ServiceEnvironment env, long handle) {
            this.env = env;
            this.handle = handle;
            this.cleanable = CLEANER.register(this, () -> release(handle));
        }

        private static void release(long h) {
            try {
                Ffi.rdds_condition_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        long handle() {
            if (closed) {
                throw Errors.closed(env, "the condition");
            }
            return handle;
        }

        void closeNative() {
            closed = true;
            cleanable.clean();
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public boolean getTriggerValue() {
            try {
                return (boolean) Ffi.rdds_condition_trigger_value.invokeExact(handle());
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    static long newCondition(ServiceEnvironment env, NativeMaker maker) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            Errors.check(env, maker.make(out.address()));
            return out.get(JAVA_LONG, 0);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    interface NativeMaker {
        int make(long out) throws Throwable;
    }

    static final class Guard extends GuardCondition {
        private final Native n;

        Guard(ServiceEnvironment env) {
            long h = newCondition(env, out -> (int) Ffi.rdds_guard_condition_new.invokeExact(out));
            n = new Native(env, h) {};
        }

        long handle() {
            return n.handle();
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return n.env;
        }

        @Override
        public boolean getTriggerValue() {
            return n.getTriggerValue();
        }

        @Override
        public void setTriggerValue(boolean value) {
            try {
                Errors.check(n.env, (int) Ffi.rdds_guard_condition_set_trigger_value.invokeExact(n.handle(), value));
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    static final class StatusCond<E extends Entity<?, ?>> extends Native implements StatusCondition<E> {
        private final E parent;

        StatusCond(ServiceEnvironment env, long handle, E parent) {
            super(env, handle);
            this.parent = parent;
        }

        @Override
        public Set<Class<? extends Status>> getEnabledStatuses() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_INT);
                Errors.check(env, (int) Ffi.rdds_status_condition_get_enabled_statuses.invokeExact(handle(),
                        out.address()));
                return StatusKinds.of(out.get(JAVA_INT, 0));
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        @Override
        public void setEnabledStatuses(Collection<Class<? extends Status>> statuses) {
            try {
                Errors.check(env, (int) Ffi.rdds_status_condition_set_enabled_statuses.invokeExact(handle(),
                        StatusKinds.mask(statuses)));
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        @Override
        public E getParent() {
            return parent;
        }
    }

    static final class Read<T> extends Native implements ReadCondition<T> {
        private final ReaderImpl<T> parent;
        final DataStateImpl states;

        Read(ServiceEnvironment env, long handle, ReaderImpl<T> parent, DataStateImpl states) {
            super(env, handle);
            this.parent = parent;
            this.states = states.clone();
        }

        @Override
        public Set<SampleState> getSampleStates() {
            return states.getSampleStates();
        }

        @Override
        public Set<ViewState> getViewStates() {
            return states.getViewStates();
        }

        @Override
        public Set<InstanceState> getInstanceStates() {
            return states.getInstanceStates();
        }

        @Override
        public DataReader<T> getParent() {
            return parent;
        }

        @Override
        public void close() {
            parent.forget(this);
            closeNative();
        }
    }

    /** The rdds_condition of a condition of this implementation. */
    static long handleOf(Condition c) {
        if (c instanceof Native n) {
            return n.handle();
        }
        if (c instanceof Guard g) {
            return g.handle();
        }
        throw new IllegalArgumentException("a condition of another DDS implementation: " + c);
    }

    static final class WaitSetImpl extends WaitSet {
        private final ServiceEnvironment env;
        private final long handle;
        // The attached conditions, in the order rdds_waitset_wait numbers
        // them.
        private final List<Condition> attached = new ArrayList<>();

        WaitSetImpl(ServiceEnvironment env) {
            this.env = env;
            this.handle = newCondition(env, out -> (int) Ffi.rdds_waitset_new.invokeExact(out));
            long h = handle;
            CLEANER.register(this, () -> {
                try {
                    Ffi.rdds_waitset_release.invokeExact(h);
                } catch (Throwable t) {
                    throw Library.rethrow(t);
                }
            });
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public void waitForConditions() {
            try {
                waitFor(null, DurationImpl.INFINITE);
            } catch (TimeoutException e) {
                throw new AssertionError(e);
            }
        }

        @Override
        public void waitForConditions(Collection<Condition> activeConditions) {
            try {
                waitFor(activeConditions, DurationImpl.INFINITE);
            } catch (TimeoutException e) {
                throw new AssertionError(e);
            }
        }

        @Override
        public void waitForConditions(Duration timeout) throws TimeoutException {
            waitFor(null, DurationImpl.nanos(timeout));
        }

        @Override
        public void waitForConditions(long timeout, TimeUnit unit) throws TimeoutException {
            waitFor(null, DurationImpl.of(env, timeout, unit).nanos);
        }

        @Override
        public void waitForConditions(Collection<Condition> activeConditions, Duration timeout)
                throws TimeoutException {
            waitFor(activeConditions, DurationImpl.nanos(timeout));
        }

        @Override
        public void waitForConditions(Collection<Condition> activeConditions, long timeout, TimeUnit unit)
                throws TimeoutException {
            waitFor(activeConditions, DurationImpl.of(env, timeout, unit).nanos);
        }

        private void waitFor(Collection<Condition> active, long nanos) throws TimeoutException {
            List<Condition> snapshot;
            synchronized (attached) {
                snapshot = new ArrayList<>(attached);
            }
            int cap = Math.max(1, snapshot.size());
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment d = arena.allocate(Ffi.DURATION);
                DurationImpl.put(d, nanos);
                MemorySegment idx = arena.allocate(JAVA_LONG, cap);
                MemorySegment len = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_waitset_wait.invokeExact(handle, d, idx.address(), (long) cap,
                        len.address());
                Errors.checkTimeout(env, rc);
                if (active != null) {
                    active.clear();
                    long n = Math.min(len.get(JAVA_LONG, 0), cap);
                    for (int i = 0; i < n; i++) {
                        long k = idx.getAtIndex(JAVA_LONG, i);
                        if (k < snapshot.size()) {
                            active.add(snapshot.get((int) k));
                        }
                    }
                }
            } catch (TimeoutException | RuntimeException | Error e) {
                throw e;
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        @Override
        public void attachCondition(Condition cond) {
            synchronized (attached) {
                if (attached.contains(cond)) {
                    return;
                }
                try {
                    Errors.check(env, (int) Ffi.rdds_waitset_attach_condition.invokeExact(handle, handleOf(cond)));
                } catch (Throwable t) {
                    throw Library.rethrow(t);
                }
                attached.add(cond);
            }
        }

        @Override
        public void detachCondition(Condition cond) {
            synchronized (attached) {
                if (!attached.contains(cond)) {
                    throw Errors.precondition(env, "the condition is not attached");
                }
                try {
                    Errors.check(env, (int) Ffi.rdds_waitset_detach_condition.invokeExact(handle, handleOf(cond)));
                } catch (Throwable t) {
                    throw Library.rethrow(t);
                }
                attached.remove(cond);
            }
        }

        @Override
        public Collection<Condition> getConditions() {
            synchronized (attached) {
                return Collections.unmodifiableList(new ArrayList<>(attached));
            }
        }
    }
}
