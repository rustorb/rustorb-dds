// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import org.omg.dds.core.status.DataAvailableStatus;
import org.omg.dds.sub.DataReader;

/** A change of the DataAvailableStatus of a DataReader. */
public abstract class DataAvailableEvent<TYPE> extends StatusChangedEvent<DataReader<TYPE>> {
    private static final long serialVersionUID = 1L;

    protected DataAvailableEvent(DataReader<TYPE> source) {
        super(source);
    }

    public abstract DataAvailableStatus getStatus();

    @Override
    public abstract DataAvailableEvent<TYPE> clone();
}
