// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import org.omg.dds.core.EntityQos;
import org.omg.dds.core.policy.Partition;
import org.omg.dds.core.policy.QosPolicy;

/** The QoS of a Subscriber. */
public interface SubscriberQos extends EntityQos<QosPolicy.ForSubscriber> {
    public Partition getPartition();

    @Override
    public SubscriberQos withPolicy(QosPolicy.ForSubscriber policy);

    @Override
    public SubscriberQos withPolicies(QosPolicy.ForSubscriber... policy);
}
