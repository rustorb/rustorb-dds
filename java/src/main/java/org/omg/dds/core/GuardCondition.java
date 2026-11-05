// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

/** A condition the application sets. */
public abstract class GuardCondition implements Condition {
    public static GuardCondition newGuardCondition(ServiceEnvironment env) {
        return env.getSPI().newGuardCondition();
    }

    public abstract void setTriggerValue(boolean value);
}
