// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import org.omg.dds.core.EntityQos;
import org.omg.dds.core.policy.DataRepresentation;
import org.omg.dds.core.policy.Deadline;
import org.omg.dds.core.policy.Durability;
import org.omg.dds.core.policy.History;
import org.omg.dds.core.policy.Liveliness;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.Reliability;
import org.omg.dds.core.policy.ResourceLimits;
import org.omg.dds.core.policy.UserData;

/** The QoS of a DataReader. */
public interface DataReaderQos extends EntityQos<QosPolicy.ForDataReader> {
    public Durability getDurability();

    public Deadline getDeadline();

    public Liveliness getLiveliness();

    public Reliability getReliability();

    public History getHistory();

    public ResourceLimits getResourceLimits();

    public UserData getUserData();

    public DataRepresentation getRepresentation();

    @Override
    public DataReaderQos withPolicy(QosPolicy.ForDataReader policy);

    @Override
    public DataReaderQos withPolicies(QosPolicy.ForDataReader... policy);
}
