// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.domain;

import org.omg.dds.core.event.DataAvailableEvent;
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

/** A DomainParticipantListener whose methods do nothing. */
public class DomainParticipantAdapter implements DomainParticipantListener {
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

    @Override
    public void onInconsistentTopic(InconsistentTopicEvent<?> status) {
    }
}
