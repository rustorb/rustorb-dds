// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core.event;

import org.omg.dds.core.status.InconsistentTopicStatus;
import org.omg.dds.topic.Topic;

/** A change of the InconsistentTopicStatus of a Topic. */
public abstract class InconsistentTopicEvent<TYPE> extends StatusChangedEvent<Topic<TYPE>> {
    private static final long serialVersionUID = 1L;

    protected InconsistentTopicEvent(Topic<TYPE> source) {
        super(source);
    }

    public abstract InconsistentTopicStatus getStatus();

    @Override
    public abstract InconsistentTopicEvent<TYPE> clone();
}
