// rustorb-dds's subset of the Java 5 Language PSM for DDS (OMG
// formal/2013-11-02), on the C ABI of rustorb-dds (see docs/java.md).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.omg.dds.sub;

import java.util.List;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.DDSObject;
import org.omg.dds.core.DomainEntity;
import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.status.LivelinessChangedStatus;
import org.omg.dds.core.status.RequestedDeadlineMissedStatus;
import org.omg.dds.core.status.RequestedIncompatibleQosStatus;
import org.omg.dds.core.status.SampleLostStatus;
import org.omg.dds.core.status.SampleRejectedStatus;
import org.omg.dds.core.status.SubscriptionMatchedStatus;
import org.omg.dds.topic.TopicDescription;

/** Reads the samples of a topic. */
public interface DataReader<TYPE> extends DomainEntity<DataReaderListener<TYPE>, DataReaderQos> {
    public <OTHER> DataReader<OTHER> cast();

    public ReadCondition<TYPE> createReadCondition(Subscriber.DataState states);

    /** Closes the read conditions. */
    public void closeContainedEntities();

    public TopicDescription<TYPE> getTopicDescription();

    public SampleRejectedStatus getSampleRejectedStatus();

    public LivelinessChangedStatus getLivelinessChangedStatus();

    public RequestedDeadlineMissedStatus getRequestedDeadlineMissedStatus();

    public RequestedIncompatibleQosStatus getRequestedIncompatibleQosStatus();

    public SubscriptionMatchedStatus getSubscriptionMatchedStatus();

    public SampleLostStatus getSampleLostStatus();

    /** rustorb-dds delivers the samples of TRANSIENT_LOCAL writers as they
     * come: this returns at once. */
    public void waitForHistoricalData(Duration maxWait) throws TimeoutException;

    public void waitForHistoricalData(long maxWait, TimeUnit unit) throws TimeoutException;

    /** The GUIDs of the matched writers. */
    public Set<InstanceHandle> getMatchedPublications();

    public Sample.Iterator<TYPE> read();

    public Sample.Iterator<TYPE> read(Selector<TYPE> query);

    public Sample.Iterator<TYPE> read(int maxSamples);

    public List<Sample<TYPE>> read(List<Sample<TYPE>> samples);

    public List<Sample<TYPE>> read(List<Sample<TYPE>> samples, Selector<TYPE> selector);

    public Sample.Iterator<TYPE> take();

    public Sample.Iterator<TYPE> take(int maxSamples);

    public Sample.Iterator<TYPE> take(Selector<TYPE> query);

    public List<Sample<TYPE>> take(List<Sample<TYPE>> samples);

    public List<Sample<TYPE>> take(List<Sample<TYPE>> samples, Selector<TYPE> query);

    public TYPE getKeyValue(TYPE keyHolder, InstanceHandle handle);

    public ModifiableInstanceHandle lookupInstance(ModifiableInstanceHandle handle, TYPE keyHolder);

    public InstanceHandle lookupInstance(TYPE keyHolder);

    @Override
    public StatusCondition<DataReader<TYPE>> getStatusCondition();

    @Override
    public Subscriber getParent();

    /** Which samples to read or take: by default every sample of every
     * instance, in any state, as many as there are. */
    public Selector<TYPE> select();

    /** Selection criteria; the methods that set one return a new
     * Selector. */
    public static interface Selector<T> extends DDSObject {
        public Selector<T> instance(InstanceHandle handle);

        public Selector<T> nextInstance(boolean retrieveNextInstance);

        public Selector<T> dataState(Subscriber.DataState state);

        /** At most max samples (LENGTH_UNLIMITED: no limit). rustorb-dds
         * applies it to read, not to take (UnsupportedOperationException). */
        public Selector<T> maxSamples(int max);

        public InstanceHandle getInstance();

        public boolean retrieveNextInstance();

        public Subscriber.DataState getDataState();

        public int getMaxSamples();

        /** The read condition whose samples the selector reads
         * (org.rustorb.dds.Rustorb.select(condition)), or null. */
        public ReadCondition<T> getCondition();

        public Sample.Iterator<T> read();

        public List<Sample<T>> read(List<Sample<T>> samples);

        public Sample.Iterator<T> take();

        public List<Sample<T>> take(List<Sample<T>> samples);
    }
}
