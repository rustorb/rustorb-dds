// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.pub;

import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.DomainEntity;
import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.Time;
import org.omg.dds.core.status.LivelinessLostStatus;
import org.omg.dds.core.status.OfferedDeadlineMissedStatus;
import org.omg.dds.core.status.OfferedIncompatibleQosStatus;
import org.omg.dds.core.status.PublicationMatchedStatus;
import org.omg.dds.topic.Topic;

/**
 * Writes the samples of a topic. The instance operations that take a
 * handle without a sample accept the handles that this writer's
 * registerInstance and lookupInstance returned (rustorb-dds names an
 * instance by the value of its key).
 */
public interface DataWriter<TYPE> extends DomainEntity<DataWriterListener<TYPE>, DataWriterQos> {
    public <OTHER> DataWriter<OTHER> cast();

    public Topic<TYPE> getTopic();

    /** Until every reliable reader acknowledged what was written. */
    public void waitForAcknowledgments(Duration maxWait) throws TimeoutException;

    public void waitForAcknowledgments(long maxWait, TimeUnit unit) throws TimeoutException;

    public LivelinessLostStatus getLivelinessLostStatus();

    public OfferedDeadlineMissedStatus getOfferedDeadlineMissedStatus();

    public OfferedIncompatibleQosStatus getOfferedIncompatibleQosStatus();

    public PublicationMatchedStatus getPublicationMatchedStatus();

    public void assertLiveliness();

    /** The GUIDs of the matched readers. */
    public Set<InstanceHandle> getMatchedSubscriptions();

    public InstanceHandle registerInstance(TYPE instanceData) throws TimeoutException;

    public InstanceHandle registerInstance(TYPE instanceData, Time sourceTimestamp) throws TimeoutException;

    public InstanceHandle registerInstance(TYPE instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException;

    public void unregisterInstance(InstanceHandle handle) throws TimeoutException;

    public void unregisterInstance(InstanceHandle handle, TYPE instanceData) throws TimeoutException;

    public void unregisterInstance(InstanceHandle handle, TYPE instanceData, Time sourceTimestamp)
            throws TimeoutException;

    public void unregisterInstance(InstanceHandle handle, TYPE instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException;

    public void write(TYPE instanceData) throws TimeoutException;

    public void write(TYPE instanceData, Time sourceTimestamp) throws TimeoutException;

    public void write(TYPE instanceData, long sourceTimestamp, TimeUnit unit) throws TimeoutException;

    public void write(TYPE instanceData, InstanceHandle handle) throws TimeoutException;

    public void write(TYPE instanceData, InstanceHandle handle, Time sourceTimestamp) throws TimeoutException;

    public void write(TYPE instanceData, InstanceHandle handle, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException;

    public void dispose(InstanceHandle instanceHandle) throws TimeoutException;

    public void dispose(InstanceHandle instanceHandle, TYPE instanceData) throws TimeoutException;

    public void dispose(InstanceHandle instanceHandle, TYPE instanceData, Time sourceTimestamp)
            throws TimeoutException;

    public void dispose(InstanceHandle instanceHandle, TYPE instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException;

    /** The key of the instance, in keyHolder (whose other members are
     * left as they are only if the type has none). */
    public TYPE getKeyValue(TYPE keyHolder, InstanceHandle handle);

    public TYPE getKeyValue(InstanceHandle handle);

    public ModifiableInstanceHandle lookupInstance(ModifiableInstanceHandle handle, TYPE keyHolder);

    public InstanceHandle lookupInstance(TYPE keyHolder);

    @Override
    public StatusCondition<DataWriter<TYPE>> getStatusCondition();

    @Override
    public Publisher getParent();
}
