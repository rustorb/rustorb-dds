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

/** The listener of a Publisher (not supported by rustorb-dds yet: setting one throws UnsupportedOperationException). */
public interface PublisherListener extends EventListener {
    public void onOfferedDeadlineMissed(OfferedDeadlineMissedEvent<?> status);

    public void onOfferedIncompatibleQos(OfferedIncompatibleQosEvent<?> status);

    public void onLivelinessLost(LivelinessLostEvent<?> status);

    public void onPublicationMatched(PublicationMatchedEvent<?> status);
}
