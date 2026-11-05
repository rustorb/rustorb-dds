// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

/** WRITER_DATA_LIFECYCLE (the method names are spelled as in the PSM). */
public interface WriterDataLifecycle extends QosPolicy.ForDataWriter {
    public boolean isAutDisposeUnregisteredInstances();

    public WriterDataLifecycle withAutDisposeUnregisteredInstances(boolean autDisposeUnregisteredInstances);
}
