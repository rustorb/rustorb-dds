// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.topic;

import org.omg.dds.core.DomainEntity;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.InconsistentTopicStatus;
import org.omg.dds.domain.DomainParticipant;

/** A topic: a name and a type. */
public interface Topic<TYPE> extends TopicDescription<TYPE>, DomainEntity<TopicListener<TYPE>, TopicQos> {
    public InconsistentTopicStatus getInconsistentTopicStatus();

    @Override
    public StatusCondition<Topic<TYPE>> getStatusCondition();

    @Override
    public DomainParticipant getParent();

    @Override
    public void close();
}
