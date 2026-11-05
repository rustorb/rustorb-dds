// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import org.omg.dds.core.EntityQos;
import org.omg.dds.core.policy.Partition;
import org.omg.dds.core.policy.QosPolicy;

/** The QoS of a Publisher. */
public interface PublisherQos extends EntityQos<QosPolicy.ForPublisher> {
    public Partition getPartition();

    @Override
    public PublisherQos withPolicy(QosPolicy.ForPublisher policy);

    @Override
    public PublisherQos withPolicies(QosPolicy.ForPublisher... policy);
}
