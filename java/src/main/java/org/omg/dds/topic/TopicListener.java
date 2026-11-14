// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.topic;

import java.util.EventListener;
import org.omg.dds.core.event.InconsistentTopicEvent;

/** The listener of a Topic. The topics of one name in a participant share their listener. */
public interface TopicListener<TYPE> extends EventListener {
    public void onInconsistentTopic(InconsistentTopicEvent<TYPE> status);
}
