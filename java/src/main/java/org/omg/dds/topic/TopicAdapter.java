// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.topic;

import org.omg.dds.core.event.InconsistentTopicEvent;

/** A TopicListener whose methods do nothing. */
public class TopicAdapter<TYPE> implements TopicListener<TYPE> {
    @Override
    public void onInconsistentTopic(InconsistentTopicEvent<TYPE> status) {
    }
}
