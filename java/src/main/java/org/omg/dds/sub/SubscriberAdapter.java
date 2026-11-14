// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.DataOnReadersEvent;
import org.omg.dds.core.event.LivelinessChangedEvent;
import org.omg.dds.core.event.RequestedDeadlineMissedEvent;
import org.omg.dds.core.event.RequestedIncompatibleQosEvent;
import org.omg.dds.core.event.SampleLostEvent;
import org.omg.dds.core.event.SampleRejectedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;

/** A SubscriberListener whose methods do nothing. */
public class SubscriberAdapter implements SubscriberListener {
    @Override
    public void onDataOnReaders(DataOnReadersEvent status) {
    }

    @Override
    public void onRequestedDeadlineMissed(RequestedDeadlineMissedEvent<?> status) {
    }

    @Override
    public void onRequestedIncompatibleQos(RequestedIncompatibleQosEvent<?> status) {
    }

    @Override
    public void onSampleRejected(SampleRejectedEvent<?> status) {
    }

    @Override
    public void onLivelinessChanged(LivelinessChangedEvent<?> status) {
    }

    @Override
    public void onDataAvailable(DataAvailableEvent<?> status) {
    }

    @Override
    public void onSubscriptionMatched(SubscriptionMatchedEvent<?> status) {
    }

    @Override
    public void onSampleLost(SampleLostEvent<?> status) {
    }
}
