// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.util.Collection;
import java.util.Set;

/** PARTITION. */
public interface Partition extends QosPolicy.ForPublisher, QosPolicy.ForSubscriber {
    public Set<String> getName();

    public Partition withName(Collection<String> name);
}
