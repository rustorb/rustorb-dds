// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import org.omg.dds.core.status.DataOnReadersStatus;
import org.omg.dds.sub.Subscriber;

/** A change of the DataOnReadersStatus of a Subscriber: some of its readers have new data. */
public abstract class DataOnReadersEvent extends StatusChangedEvent<Subscriber> {
    private static final long serialVersionUID = 1L;

    protected DataOnReadersEvent(Subscriber source) {
        super(source);
    }

    public abstract DataOnReadersStatus getStatus();

    @Override
    public abstract DataOnReadersEvent clone();
}
