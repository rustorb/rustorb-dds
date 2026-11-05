// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import org.omg.dds.core.status.OfferedDeadlineMissedStatus;
import org.omg.dds.pub.DataWriter;

/** A change of the OfferedDeadlineMissedStatus of a DataWriter. */
public abstract class OfferedDeadlineMissedEvent<TYPE> extends StatusChangedEvent<DataWriter<TYPE>> {
    private static final long serialVersionUID = 1L;

    protected OfferedDeadlineMissedEvent(DataWriter<TYPE> source) {
        super(source);
    }

    public abstract OfferedDeadlineMissedStatus getStatus();

    @Override
    public abstract OfferedDeadlineMissedEvent<TYPE> clone();
}
