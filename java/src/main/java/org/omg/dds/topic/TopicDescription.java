// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.topic;

import java.io.Closeable;
import org.omg.dds.core.DDSObject;
import org.omg.dds.domain.DomainParticipant;
import org.omg.dds.type.TypeSupport;

/** What readers read: a Topic. */
public interface TopicDescription<TYPE> extends Closeable, DDSObject {
    public TypeSupport<TYPE> getTypeSupport();

    public <OTHER> TopicDescription<OTHER> cast();

    public String getTypeName();

    public String getName();

    public DomainParticipant getParent();

    @Override
    public void close();
}
