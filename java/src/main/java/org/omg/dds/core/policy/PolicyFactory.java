// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import org.omg.dds.core.DDSObject;
import org.omg.dds.core.ServiceEnvironment;

/** Makes the policies, with their default values. */
public abstract class PolicyFactory implements DDSObject {
    public static PolicyFactory getPolicyFactory(ServiceEnvironment env) {
        return env.getSPI().getPolicyFactory();
    }

    public abstract Durability Durability();

    public abstract Deadline Deadline();

    public abstract Liveliness Liveliness();

    public abstract History History();

    public abstract ResourceLimits ResourceLimits();

    public abstract UserData UserData();

    /** An empty PROPERTY (rustorb-dds; see {@link Property}). */
    public abstract Property Property();

    public abstract DataRepresentation Representation();

    public abstract Reliability Reliability();

    public abstract WriterDataLifecycle WriterDataLifecycle();

    public abstract Partition Partition();
}
