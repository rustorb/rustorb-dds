// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.policy;

import java.io.Serializable;
import org.omg.dds.core.DDSObject;

/** A QoS policy. Immutable: its with methods make new policies. The
 * nested interfaces say which entities a policy applies to. */
public interface QosPolicy extends Serializable, DDSObject {
    public static interface ForDomainParticipant extends QosPolicy {
    }

    public static interface ForPublisher extends QosPolicy {
    }

    public static interface ForSubscriber extends QosPolicy {
    }

    public static interface ForTopic extends QosPolicy {
    }

    public static interface ForDataWriter extends QosPolicy {
    }

    public static interface ForDataReader extends QosPolicy {
    }
}
