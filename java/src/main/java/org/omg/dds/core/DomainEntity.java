// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.util.EventListener;

/** An entity that belongs to a DomainParticipant. */
public interface DomainEntity<LISTENER extends EventListener, QOS extends EntityQos<?>> extends Entity<LISTENER, QOS> {
    public Entity<?, ?> getParent();
}
