// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import java.util.EventObject;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.Entity;

/** A change of a status of an entity, given to a listener. */
public abstract class StatusChangedEvent<SOURCE extends Entity<?, ?>> extends EventObject
        implements Cloneable, DDSObject {
    private static final long serialVersionUID = 1L;

    protected StatusChangedEvent(SOURCE source) {
        super(source);
    }

    @Override
    public abstract SOURCE getSource();

    @Override
    public abstract StatusChangedEvent<SOURCE> clone();

    protected void setSource(SOURCE source) {
        super.source = source;
    }
}
