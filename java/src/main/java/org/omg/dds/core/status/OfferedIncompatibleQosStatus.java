// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.status;

import java.util.Set;
import org.omg.dds.core.policy.QosPolicy;
import org.omg.dds.core.policy.QosPolicyCount;

/** The OFFERED_INCOMPATIBLE_QOS status. */
public abstract class OfferedIncompatibleQosStatus extends Status {
    private static final long serialVersionUID = 1L;

    public abstract int getTotalCount();

    public abstract int getTotalCountChange();

    public abstract Class<? extends QosPolicy> getLastPolicyClass();

    public abstract Set<QosPolicyCount> getPolicies();
}
