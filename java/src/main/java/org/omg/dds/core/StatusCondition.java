// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.util.Collection;
import java.util.Set;
import org.omg.dds.core.status.Status;

/** The condition of the statuses of an entity. */
public interface StatusCondition<ENTITY extends Entity<?, ?>> extends Condition {
    public Set<Class<? extends Status>> getEnabledStatuses();

    public void setEnabledStatuses(Collection<Class<? extends Status>> statuses);

    public ENTITY getParent();
}
