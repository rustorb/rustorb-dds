// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.status;

/** The SAMPLE_LOST status. */
public abstract class SampleLostStatus extends Status {
    private static final long serialVersionUID = 1L;

    public abstract int getTotalCount();

    public abstract int getTotalCountChange();
}
