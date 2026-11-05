// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import java.util.EventListener;
import org.omg.dds.core.event.LivelinessLostEvent;
import org.omg.dds.core.event.OfferedDeadlineMissedEvent;
import org.omg.dds.core.event.OfferedIncompatibleQosEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;

/** The listener of a DataWriter. rustorb-dds calls it on a thread of its own, or, for a reader of the same participant, inside the write that caused it. */
public interface DataWriterListener<TYPE> extends EventListener {
    public void onOfferedDeadlineMissed(OfferedDeadlineMissedEvent<TYPE> status);

    public void onOfferedIncompatibleQos(OfferedIncompatibleQosEvent<TYPE> status);

    public void onLivelinessLost(LivelinessLostEvent<TYPE> status);

    public void onPublicationMatched(PublicationMatchedEvent<TYPE> status);
}
