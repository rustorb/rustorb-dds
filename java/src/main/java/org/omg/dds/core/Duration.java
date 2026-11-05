// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.io.Serializable;
import java.util.concurrent.TimeUnit;

/** A Duration_t: a span of time, or infinity. Immutable. */
public abstract class Duration implements Comparable<Duration>, Serializable, DDSObject {
    private static final long serialVersionUID = 1L;

    public static Duration newDuration(long duration, TimeUnit unit, ServiceEnvironment env) {
        return env.getSPI().newDuration(duration, unit);
    }

    public static Duration infiniteDuration(ServiceEnvironment env) {
        return env.getSPI().infiniteDuration();
    }

    public static Duration zeroDuration(ServiceEnvironment env) {
        return env.getSPI().zeroDuration();
    }

    /** The duration in the unit, truncated (Long.MAX_VALUE if infinite). */
    public abstract long getDuration(TimeUnit inThisUnit);

    /** What getDuration(primaryUnit) leaves out, in remainderUnit. */
    public abstract long getRemainder(TimeUnit primaryUnit, TimeUnit remainderUnit);

    public abstract boolean isZero();

    public abstract boolean isInfinite();

    public abstract Duration add(Duration duration);

    public abstract Duration add(long duration, TimeUnit unit);

    public abstract Duration subtract(Duration duration);

    public abstract Duration subtract(long duration, TimeUnit unit);
}
