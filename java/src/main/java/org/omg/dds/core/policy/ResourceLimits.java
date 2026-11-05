// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

/** RESOURCE_LIMITS. */
public interface ResourceLimits extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public static final int LENGTH_UNLIMITED = -1;

    public int getMaxSamples();

    public int getMaxInstances();

    public int getMaxSamplesPerInstance();

    public ResourceLimits withMaxSamples(int maxSamples);

    public ResourceLimits withMaxInstances(int maxInstances);

    public ResourceLimits withMaxSamplesPerInstance(int maxSamplesPerInstance);
}
