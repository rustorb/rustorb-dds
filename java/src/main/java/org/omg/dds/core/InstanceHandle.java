// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.io.Serializable;

/** An InstanceHandle_t: names an instance or an entity. */
public abstract class InstanceHandle implements Comparable<InstanceHandle>, Serializable, DDSObject {
    private static final long serialVersionUID = 1L;

    public static ModifiableInstanceHandle newInstanceHandle(ServiceEnvironment env) {
        return env.getSPI().newInstanceHandle();
    }

    public static InstanceHandle nilHandle(ServiceEnvironment env) {
        return env.getSPI().nilHandle();
    }

    public abstract boolean isNil();

    public abstract ModifiableInstanceHandle modifiableCopy();
}
