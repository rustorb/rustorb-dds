// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.concurrent.TimeUnit;
import org.omg.dds.core.Duration;

/** RELIABILITY. */
public interface Reliability extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public Kind getKind();

    public Duration getMaxBlockingTime();

    public Reliability withKind(Kind kind);

    public Reliability withMaxBlockingTime(Duration maxBlockingTime);

    public Reliability withMaxBlockingTime(long maxBlockingTime, TimeUnit unit);

    public Reliability withBestEffort();

    public Reliability withReliable();

    public enum Kind {
        BEST_EFFORT,
        RELIABLE
    }
}
