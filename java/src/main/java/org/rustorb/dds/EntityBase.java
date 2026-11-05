// What the entities of this implementation share.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.EventListener;

import org.omg.dds.core.Entity;
import org.omg.dds.core.EntityQos;
import org.omg.dds.core.ServiceEnvironment;

abstract class EntityBase<L extends EventListener, Q extends EntityQos<?>> implements Entity<L, Q> {
    final ServiceEnvironment env;
    final String what;
    private volatile long handle;

    EntityBase(ServiceEnvironment env, String what, long handle) {
        this.env = env;
        this.what = what;
        this.handle = handle;
    }

    /** The native handle; AlreadyClosedException once closed. */
    long handle() {
        long h = handle;
        if (h == 0) {
            throw Errors.closed(env, what);
        }
        return h;
    }

    boolean isClosed() {
        return handle == 0;
    }

    /** Clears the handle; returns the old one (0 if it was closed). */
    synchronized long takeHandle() {
        long h = handle;
        handle = 0;
        return h;
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public void setQos(Q qos) {
        throw new UnsupportedOperationException("rustorb-dds cannot change the QoS of a " + what + " once it is made");
    }

    @Override
    public void enable() {
        handle();
    }

    @Override
    public void retain() {
    }

    /** Calls a C function that makes something into its output pointer. */
    static long make(ServiceEnvironment env, Maker maker) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            Errors.check(env, maker.make(arena, out.address()));
            return out.get(JAVA_LONG, 0);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    interface Maker {
        int make(Arena arena, long out) throws Throwable;
    }

    /** A C function's status, checked. */
    static void call(ServiceEnvironment env, Call c) {
        try {
            Errors.check(env, c.call());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    interface Call {
        int call() throws Throwable;
    }
}
