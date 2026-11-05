// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.domain;

import java.util.Collection;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.status.Status;

/** Makes DomainParticipants; one per ServiceEnvironment. */
public abstract class DomainParticipantFactory implements DDSObject {
    public static DomainParticipantFactory getInstance(ServiceEnvironment env) {
        return env.getSPI().getParticipantFactory();
    }

    /** A participant in domain 0. */
    public abstract DomainParticipant createParticipant();

    public abstract DomainParticipant createParticipant(int domainId);

    /** listener must be null (participant listeners are not supported yet). */
    public abstract DomainParticipant createParticipant(
            int domainId,
            DomainParticipantQos qos,
            DomainParticipantListener listener,
            Collection<Class<? extends Status>> statuses);

    /** A participant of this factory in the domain, or null. */
    public abstract DomainParticipant lookupParticipant(int domainId);

    public abstract DomainParticipantQos getDefaultParticipantQos();

    public abstract void setDefaultParticipantQos(DomainParticipantQos qos);
}
