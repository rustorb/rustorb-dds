// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.status;

import org.omg.dds.core.InstanceHandle;

/** The SAMPLE_REJECTED status. */
public abstract class SampleRejectedStatus extends Status {
    private static final long serialVersionUID = 1L;

    public abstract int getTotalCount();

    public abstract int getTotalCountChange();

    public abstract Kind getLastReason();

    public abstract InstanceHandle getLastInstanceHandle();

    /** Why a sample was rejected. */
    public static enum Kind {
        NOT_REJECTED,
        REJECTED_BY_INSTANCES_LIMIT,
        REJECTED_BY_SAMPLES_LIMIT,
        REJECTED_BY_SAMPLES_PER_INSTANCE_LIMIT
    }
}
