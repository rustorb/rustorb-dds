// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.concurrent.TimeUnit;
import org.omg.dds.core.Duration;

/** LIVELINESS. */
public interface Liveliness extends QosPolicy.ForTopic, QosPolicy.ForDataReader, QosPolicy.ForDataWriter {
    public Kind getKind();

    public Duration getLeaseDuration();

    public Liveliness withKind(Kind kind);

    public Liveliness withLeaseDuration(Duration leaseDuration);

    public Liveliness withLeaseDuration(long leaseDuration, TimeUnit unit);

    public Liveliness withAutomatic();

    public Liveliness withManualByParticipant();

    public Liveliness withManualByTopic();

    public enum Kind {
        AUTOMATIC,
        MANUAL_BY_PARTICIPANT,
        MANUAL_BY_TOPIC
    }
}
