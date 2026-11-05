// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.util.EventListener;
import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.LivelinessChangedEvent;
import org.omg.dds.core.event.RequestedDeadlineMissedEvent;
import org.omg.dds.core.event.RequestedIncompatibleQosEvent;
import org.omg.dds.core.event.SampleLostEvent;
import org.omg.dds.core.event.SampleRejectedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;

/** The listener of a DataReader. rustorb-dds calls it on a thread of its own, or, for a writer of the same participant, inside its write. */
public interface DataReaderListener<TYPE> extends EventListener {
    public void onRequestedDeadlineMissed(RequestedDeadlineMissedEvent<TYPE> status);

    public void onRequestedIncompatibleQos(RequestedIncompatibleQosEvent<TYPE> status);

    public void onSampleRejected(SampleRejectedEvent<TYPE> status);

    public void onLivelinessChanged(LivelinessChangedEvent<TYPE> status);

    public void onDataAvailable(DataAvailableEvent<TYPE> status);

    public void onSubscriptionMatched(SubscriptionMatchedEvent<TYPE> status);

    public void onSampleLost(SampleLostEvent<TYPE> status);
}
