// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import org.omg.dds.core.status.PublicationMatchedStatus;
import org.omg.dds.pub.DataWriter;

/** A change of the PublicationMatchedStatus of a DataWriter. */
public abstract class PublicationMatchedEvent<TYPE> extends StatusChangedEvent<DataWriter<TYPE>> {
    private static final long serialVersionUID = 1L;

    protected PublicationMatchedEvent(DataWriter<TYPE> source) {
        super(source);
    }

    public abstract PublicationMatchedStatus getStatus();

    @Override
    public abstract PublicationMatchedEvent<TYPE> clone();
}
