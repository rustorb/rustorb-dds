// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.status;

import org.omg.dds.core.InstanceHandle;

/** The LIVELINESS_CHANGED status. */
public abstract class LivelinessChangedStatus extends Status {
    private static final long serialVersionUID = 1L;

    public abstract int getAliveCount();

    public abstract int getNotAliveCount();

    public abstract int getAliveCountChange();

    public abstract int getNotAliveCountChange();

    public abstract InstanceHandle getLastPublicationHandle();
}
