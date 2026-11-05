// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.io.Serializable;
import org.omg.dds.core.DDSObject;

/** How many times a policy was found incompatible. */
public interface QosPolicyCount extends Serializable, DDSObject {
    public Class<? extends QosPolicy> getPolicyClass();

    public int getCount();
}
