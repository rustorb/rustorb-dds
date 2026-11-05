// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.util.Collection;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/** Waits for conditions to become true (the DDS wait is
 * waitForConditions, as Object.wait is taken). */
public abstract class WaitSet implements DDSObject {
    public static WaitSet newWaitSet(ServiceEnvironment env) {
        return env.getSPI().newWaitSet();
    }

    public abstract void waitForConditions();

    public abstract void waitForConditions(Collection<Condition> activeConditions);

    public abstract void waitForConditions(Duration timeout) throws TimeoutException;

    public abstract void waitForConditions(long timeout, TimeUnit unit) throws TimeoutException;

    public abstract void waitForConditions(Collection<Condition> activeConditions, Duration timeout)
            throws TimeoutException;

    public abstract void waitForConditions(Collection<Condition> activeConditions, long timeout, TimeUnit unit)
            throws TimeoutException;

    public abstract void attachCondition(Condition cond);

    public abstract void detachCondition(Condition cond);

    public abstract Collection<Condition> getConditions();
}
