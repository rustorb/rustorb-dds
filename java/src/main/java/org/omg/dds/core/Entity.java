// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.core;

import java.io.Closeable;
import java.util.Collection;
import java.util.EventListener;
import java.util.Set;
import org.omg.dds.core.status.Status;

/** A DDS entity, parameterized by its listener and QoS types. */
public interface Entity<LISTENER extends EventListener, QOS extends EntityQos<?>> extends Closeable, DDSObject {
    public LISTENER getListener();

    /** Sets the listener for every status. */
    public void setListener(LISTENER listener);

    /** Sets the listener for the statuses given (null: every status). */
    public void setListener(LISTENER listener, Collection<Class<? extends Status>> statuses);

    public QOS getQos();

    /** rustorb-dds cannot change the QoS of an entity after it is made:
     * throws UnsupportedOperationException. */
    public void setQos(QOS qos);

    /** Entities are enabled when they are made; this does nothing. */
    public void enable();

    public StatusCondition<?> getStatusCondition();

    public Set<Class<? extends Status>> getStatusChanges();

    public InstanceHandle getInstanceHandle();

    /** Deletes the entity and the entities it contains. */
    @Override
    public void close();

    /** Does nothing: entities live until they are closed. */
    public void retain();
}
