// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.concurrent.TimeUnit;

import org.omg.dds.core.Duration;

/** DURABILITY_SERVICE: what the participant keeps of a TRANSIENT or
 * PERSISTENT writer once it is deleted. */
public interface DurabilityService extends QosPolicy.ForDataWriter {
    public Duration getServiceCleanupDelay();

    public History.Kind getHistoryKind();

    public int getHistoryDepth();

    public int getMaxSamples();

    public int getMaxInstances();

    public int getMaxSamplesPerInstance();

    public DurabilityService withServiceCleanupDelay(Duration serviceCleanupDelay);

    public DurabilityService withServiceCleanupDelay(long serviceCleanupDelay, TimeUnit unit);

    public DurabilityService withHistoryKind(History.Kind historyKind);

    public DurabilityService withHistoryDepth(int historyDepth);

    public DurabilityService withMaxSamples(int maxSamples);

    public DurabilityService withMaxInstances(int maxInstances);

    public DurabilityService withMaxSamplesPerInstance(int maxSamplesPerInstance);
}
