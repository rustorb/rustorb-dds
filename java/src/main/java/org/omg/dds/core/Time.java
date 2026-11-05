// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.io.Serializable;
import java.util.concurrent.TimeUnit;

/** A Time_t: a point in time since 1970-01-01T00:00:00Z, or invalid. */
public abstract class Time implements Comparable<Time>, Serializable, DDSObject {
    private static final long serialVersionUID = 1L;

    public static ModifiableTime newTime(long time, TimeUnit units, ServiceEnvironment env) {
        return env.getSPI().newTime(time, units);
    }

    public static Time invalidTime(ServiceEnvironment env) {
        return env.getSPI().invalidTime();
    }

    public abstract long getTime(TimeUnit inThisUnit);

    public abstract long getRemainder(TimeUnit primaryUnit, TimeUnit remainderUnit);

    public abstract boolean isValid();

    public abstract ModifiableTime modifiableCopy();
}
