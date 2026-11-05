// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.domain;

import org.omg.dds.core.EntityQos;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.UserData;

/** The QoS of a DomainParticipant. */
public interface DomainParticipantQos extends EntityQos<QosPolicy.ForDomainParticipant> {
    public UserData getUserData();

    @Override
    public DomainParticipantQos withPolicy(QosPolicy.ForDomainParticipant policy);

    @Override
    public DomainParticipantQos withPolicies(QosPolicy.ForDomainParticipant... policy);
}
