// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.concurrent.TimeUnit;
import org.omg.dds.core.Duration;

/** DEADLINE. */
public interface Deadline extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public Duration getPeriod();

    public Deadline withPeriod(Duration period);

    public Deadline withPeriod(long period, TimeUnit unit);
}
