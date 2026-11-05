// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.io.Serializable;
import java.util.Map;
import org.omg.dds.core.policy.PolicyFactory;
import org.omg.dds.core.policy.QosPolicy;

/** The QoS of an entity: an immutable map from policy class to policy.
 * The with methods make new QoS objects. */
public interface EntityQos<P extends QosPolicy> extends Map<Class<? extends P>, P>, Serializable, DDSObject {
    public <POLICY extends P> POLICY get(Class<POLICY> id);

    public EntityQos<P> withPolicy(P policy);

    @SuppressWarnings("unchecked")
    public EntityQos<P> withPolicies(P... policy);

    public PolicyFactory getPolicyFactory();
}
