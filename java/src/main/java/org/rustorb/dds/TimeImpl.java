// The Time of the PSM, as the C ABI's rdds_time.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;

import java.lang.foreign.MemorySegment;
import java.util.concurrent.TimeUnit;

import org.omg.dds.core.Duration;
import org.omg.dds.core.ModifiableTime;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.Time;

/** A time in nanoseconds since the epoch; invalid is Long.MIN_VALUE. */
final class TimeImpl extends ModifiableTime {
    private static final long serialVersionUID = 1L;

    static final long INVALID = Long.MIN_VALUE;

    private final transient ServiceEnvironment env;
    private long nanos;

    TimeImpl(ServiceEnvironment env, long nanos) {
        this.env = env;
        this.nanos = nanos;
    }

    static TimeImpl now(ServiceEnvironment env) {
        java.time.Instant i = java.time.Instant.now();
        return new TimeImpl(env, i.getEpochSecond() * 1_000_000_000L + i.getNano());
    }

    static long nanos(Time t) {
        if (t == null) {
            throw new IllegalArgumentException("a null Time");
        }
        if (t instanceof TimeImpl i) {
            return i.nanos;
        }
        if (!t.isValid()) {
            return INVALID;
        }
        return t.getTime(TimeUnit.NANOSECONDS);
    }

    /** Writes the rdds_time of t at s; IllegalArgumentException if t is
     * invalid or before the epoch. */
    static void put(MemorySegment s, long n) {
        if (n == INVALID || n < 0) {
            throw new IllegalArgumentException("a source timestamp must be a valid time after 1970");
        }
        long sec = n / 1_000_000_000L;
        if (sec > Integer.MAX_VALUE) {
            throw new IllegalArgumentException("a time after 2038 cannot be a Time_t");
        }
        s.set(JAVA_INT, 0, (int) sec);
        s.set(JAVA_INT, 4, (int) (n % 1_000_000_000L));
    }

    static long get(MemorySegment s, long offset) {
        int sec = s.get(JAVA_INT, offset);
        int nsec = s.get(JAVA_INT, offset + 4);
        if (sec == -1 && nsec == 0xffffffff) {
            return INVALID;
        }
        return sec * 1_000_000_000L + Integer.toUnsignedLong(nsec);
    }

    @Override
    public ServiceEnvironment getEnvironment() {
        return env;
    }

    @Override
    public long getTime(TimeUnit inThisUnit) {
        return inThisUnit.convert(nanos, TimeUnit.NANOSECONDS);
    }

    @Override
    public long getRemainder(TimeUnit primaryUnit, TimeUnit remainderUnit) {
        long whole = TimeUnit.NANOSECONDS.convert(primaryUnit.convert(nanos, TimeUnit.NANOSECONDS), primaryUnit);
        return remainderUnit.convert(nanos - whole, TimeUnit.NANOSECONDS);
    }

    @Override
    public boolean isValid() {
        return nanos != INVALID;
    }

    @Override
    public ModifiableTime modifiableCopy() {
        return new TimeImpl(env, nanos);
    }

    @Override
    public void copyFrom(Time src) {
        nanos = nanos(src);
    }

    @Override
    public Time immutableCopy() {
        return new TimeImpl(env, nanos);
    }

    @Override
    public void setTime(long time, TimeUnit unit) {
        nanos = unit.toNanos(time);
    }

    @Override
    public void add(Duration duration) {
        nanos += DurationImpl.nanos(duration);
    }

    @Override
    public void add(long duration, TimeUnit unit) {
        nanos += unit.toNanos(duration);
    }

    @Override
    public void subtract(Duration duration) {
        nanos -= DurationImpl.nanos(duration);
    }

    @Override
    public void subtract(long duration, TimeUnit unit) {
        nanos -= unit.toNanos(duration);
    }

    @Override
    public int compareTo(Time o) {
        return Long.compare(nanos, nanos(o));
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof Time t && nanos == nanos(t);
    }

    @Override
    public int hashCode() {
        return Long.hashCode(nanos);
    }

    @Override
    public String toString() {
        return nanos == INVALID ? "Time(invalid)" : "Time(" + nanos + " ns)";
    }
}
