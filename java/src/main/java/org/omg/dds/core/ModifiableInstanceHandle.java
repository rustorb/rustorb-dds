// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** An InstanceHandle that can be changed. */
public abstract class ModifiableInstanceHandle extends InstanceHandle {
    private static final long serialVersionUID = 1L;

    public abstract void copyFrom(InstanceHandle src);

    public abstract InstanceHandle immutableCopy();
}
