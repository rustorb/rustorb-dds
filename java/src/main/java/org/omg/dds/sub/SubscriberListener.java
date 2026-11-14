// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.util.EventListener;
import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.DataOnReadersEvent;
import org.omg.dds.core.event.LivelinessChangedEvent;
import org.omg.dds.core.event.RequestedDeadlineMissedEvent;
import org.omg.dds.core.event.RequestedIncompatibleQosEvent;
import org.omg.dds.core.event.SampleLostEvent;
import org.omg.dds.core.event.SampleRejectedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;

/**
 * The listener of a Subscriber: DATA_ON_READERS, and the statuses of its readers that their own listeners do not
 * take. The source of an event of a reader is null for a reader not made through this binding.
 */
public interface SubscriberListener extends EventListener {
    /** Some of its readers have new data; then no onDataAvailable is called (see Subscriber.notifyDataReaders). */
    public void onDataOnReaders(DataOnReadersEvent status);

    public void onRequestedDeadlineMissed(RequestedDeadlineMissedEvent<?> status);

    public void onRequestedIncompatibleQos(RequestedIncompatibleQosEvent<?> status);

    public void onSampleRejected(SampleRejectedEvent<?> status);

    public void onLivelinessChanged(LivelinessChangedEvent<?> status);

    public void onDataAvailable(DataAvailableEvent<?> status);

    public void onSubscriptionMatched(SubscriptionMatchedEvent<?> status);

    public void onSampleLost(SampleLostEvent<?> status);
}
