// Duration and Time of the PSM, as the C ABI's rdds_duration / rdds_time.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;

import java.lang.foreign.MemorySegment;
import java.util.concurrent.TimeUnit;

import org.omg.dds.core.Duration;
import org.omg.dds.core.ServiceEnvironment;

/** A Duration in nanoseconds; infinite is Long.MAX_VALUE. */
final class DurationImpl extends Duration {
    private static final long serialVersionUID = 1L;

    static final long INFINITE = Long.MAX_VALUE;
    static final int INFINITE_SEC = 0x7fffffff;
    static final int INFINITE_NSEC = 0x7fffffff;

    private final transient ServiceEnvironment env;
    final long nanos;

    DurationImpl(ServiceEnvironment env, long nanos) {
        this.env = env;
        this.nanos = nanos < 0 ? 0 : nanos;
    }

    static DurationImpl of(ServiceEnvironment env, long duration, TimeUnit unit) {
        if (duration == Long.MAX_VALUE) {
            return new DurationImpl(env, INFINITE);
        }
        if (duration < 0) {
            throw new IllegalArgumentException("a negative duration");
        }
        long n = unit.toNanos(duration);
        return new DurationImpl(env, n);
    }

    /** The nanoseconds of any Duration (of this implementation or not). */
    static long nanos(Duration d) {
        if (d == null) {
            throw new IllegalArgumentException("a null Duration");
        }
        if (d instanceof DurationImpl i) {
            return i.nanos;
        }
        if (d.isInfinite()) {
            return INFINITE;
        }
        return d.getDuration(TimeUnit.NANOSECONDS);
    }

    /** Writes the rdds_duration of n nanoseconds at s. */
    static void put(MemorySegment s, long n) {
        if (n == INFINITE || n / 1_000_000_000L >= INFINITE_SEC) {
            s.set(JAVA_INT, 0, INFINITE_SEC);
            s.set(JAVA_INT, 4, INFINITE_NSEC);
        } else {
            s.set(JAVA_INT, 0, (int) (n / 1_000_000_000L));
            s.set(JAVA_INT, 4, (int) (n % 1_000_000_000L));
        }
    }

    /** The nanoseconds of the rdds_duration at s. */
    static long get(MemorySegment s, long offset) {
        int sec = s.get(JAVA_INT, offset);
        int nsec = s.get(JAVA_INT, offset + 4);
        if (sec == INFINITE_SEC && nsec == INFINITE_NSEC) {
            return INFINITE;
        }
        return sec * 1_000_000_000L + Integer.toUnsignedLong(nsec);
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public long getDuration(TimeUnit inThisUnit) {
        if (nanos == INFINITE) {
            return Long.MAX_VALUE;
        }
        return inThisUnit.convert(nanos, TimeUnit.NANOSECONDS);
    }

    @Override
    public long getRemainder(TimeUnit primaryUnit, TimeUnit remainderUnit) {
        if (nanos == INFINITE) {
            return 0;
        }
        long whole = TimeUnit.NANOSECONDS.convert(primaryUnit.convert(nanos, TimeUnit.NANOSECONDS), primaryUnit);
        return remainderUnit.convert(nanos - whole, TimeUnit.NANOSECONDS);
    }

    @Override
    public boolean isZero() {
        return nanos == 0;
    }

    @Override
    public boolean isInfinite() {
        return nanos == INFINITE;
    }

    @Override
    public Duration add(Duration duration) {
        return plus(nanos(duration));
    }

    @Override
    public Duration add(long duration, TimeUnit unit) {
        return plus(nanos(of(env, duration, unit)));
    }

    @Override
    public Duration subtract(Duration duration) {
        return minus(nanos(duration));
    }

    @Override
    public Duration subtract(long duration, TimeUnit unit) {
        return minus(nanos(of(env, duration, unit)));
    }

    private Duration plus(long n) {
        if (nanos == INFINITE || n == INFINITE) {
            return new DurationImpl(env, INFINITE);
        }
        long r = nanos + n;
        return new DurationImpl(env, r < 0 ? INFINITE : r);
    }

    private Duration minus(long n) {
        if (n == INFINITE) {
            throw new IllegalArgumentException("cannot subtract an infinite duration");
        }
        if (nanos == INFINITE) {
            return this;
        }
        if (n > nanos) {
            throw new IllegalArgumentException("the result would be negative");
        }
        return new DurationImpl(env, nanos - n);
    }

    @Override
    public int compareTo(Duration o) {
        return Long.compare(nanos, nanos(o));
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof Duration d && nanos == nanos(d);
    }

    @Override
    public int hashCode() {
        return Long.hashCode(nanos);
    }

    @Override
    public String toString() {
        return nanos == INFINITE ? "Duration(infinite)" : "Duration(" + nanos + " ns)";
    }
}
