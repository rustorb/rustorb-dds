// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import org.omg.dds.core.event.LivelinessLostEvent;
import org.omg.dds.core.event.OfferedDeadlineMissedEvent;
import org.omg.dds.core.event.OfferedIncompatibleQosEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;

/** A PublisherListener whose methods do nothing. */
public class PublisherAdapter implements PublisherListener {
    @Override
    public void onOfferedDeadlineMissed(OfferedDeadlineMissedEvent<?> status) {
    }

    @Override
    public void onOfferedIncompatibleQos(OfferedIncompatibleQosEvent<?> status) {
    }

    @Override
    public void onLivelinessLost(LivelinessLostEvent<?> status) {
    }

    @Override
    public void onPublicationMatched(PublicationMatchedEvent<?> status) {
    }
}
