// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.domain;

import java.util.EventListener;
import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.DataOnReadersEvent;
import org.omg.dds.core.event.InconsistentTopicEvent;
import org.omg.dds.core.event.LivelinessChangedEvent;
import org.omg.dds.core.event.LivelinessLostEvent;
import org.omg.dds.core.event.OfferedDeadlineMissedEvent;
import org.omg.dds.core.event.OfferedIncompatibleQosEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;
import org.omg.dds.core.event.RequestedDeadlineMissedEvent;
import org.omg.dds.core.event.RequestedIncompatibleQosEvent;
import org.omg.dds.core.event.SampleLostEvent;
import org.omg.dds.core.event.SampleRejectedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;

/** The listener of a DomainParticipant: what the listeners of its entities do not take. */
public interface DomainParticipantListener extends EventListener {
    public void onOfferedDeadlineMissed(OfferedDeadlineMissedEvent<?> status);

    public void onOfferedIncompatibleQos(OfferedIncompatibleQosEvent<?> status);

    public void onLivelinessLost(LivelinessLostEvent<?> status);

    public void onPublicationMatched(PublicationMatchedEvent<?> status);

    public void onRequestedDeadlineMissed(RequestedDeadlineMissedEvent<?> status);

    public void onRequestedIncompatibleQos(RequestedIncompatibleQosEvent<?> status);

    public void onSampleRejected(SampleRejectedEvent<?> status);

    public void onLivelinessChanged(LivelinessChangedEvent<?> status);

    public void onDataOnReaders(DataOnReadersEvent status);

    public void onDataAvailable(DataAvailableEvent<?> status);

    public void onSubscriptionMatched(SubscriptionMatchedEvent<?> status);

    public void onSampleLost(SampleLostEvent<?> status);

    public void onInconsistentTopic(InconsistentTopicEvent<?> status);
}
