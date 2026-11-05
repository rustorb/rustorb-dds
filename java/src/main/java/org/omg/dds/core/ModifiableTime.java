// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.util.concurrent.TimeUnit;

/** A Time that can be changed. */
public abstract class ModifiableTime extends Time {
    private static final long serialVersionUID = 1L;

    public abstract void copyFrom(Time src);

    public abstract Time immutableCopy();

    public abstract void setTime(long time, TimeUnit unit);

    public abstract void add(Duration duration);

    public abstract void add(long duration, TimeUnit unit);

    public abstract void subtract(Duration duration);

    public abstract void subtract(long duration, TimeUnit unit);
}
