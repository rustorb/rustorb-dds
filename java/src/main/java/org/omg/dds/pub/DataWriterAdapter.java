// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import org.omg.dds.core.event.LivelinessLostEvent;
import org.omg.dds.core.event.OfferedDeadlineMissedEvent;
import org.omg.dds.core.event.OfferedIncompatibleQosEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;

/** A DataWriterListener whose methods do nothing. */
public class DataWriterAdapter<TYPE> implements DataWriterListener<TYPE> {
    @Override
    public void onOfferedDeadlineMissed(OfferedDeadlineMissedEvent<TYPE> status) {
    }

    @Override
    public void onOfferedIncompatibleQos(OfferedIncompatibleQosEvent<TYPE> status) {
    }

    @Override
    public void onLivelinessLost(LivelinessLostEvent<TYPE> status) {
    }

    @Override
    public void onPublicationMatched(PublicationMatchedEvent<TYPE> status) {
    }
}
