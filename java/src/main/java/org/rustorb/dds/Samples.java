// The samples a reader returns, and the Selector of the PSM.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_INT;

import java.lang.foreign.MemorySegment;
import java.util.List;
import java.util.ListIterator;
import java.util.NoSuchElementException;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.core.Time;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.InstanceState;
import org.omg.dds.sub.ReadCondition;
import org.omg.dds.sub.Sample;
import org.omg.dds.sub.SampleState;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.sub.ViewState;

final class Samples {
    private Samples() {}

    /** The size of rdds_sample_info. */
    static final long INFO_SIZE = 88;

    static final class SampleImpl<T> implements Sample<T> {
        private static final long serialVersionUID = 1L;
        private final transient ServiceEnvironment env;
        private final T data;
        private final SampleState sampleState;
        private final ViewState viewState;
        private final InstanceState instanceState;
        private final Time sourceTimestamp;
        private final HandleImpl instance;
        private final HandleImpl publication;
        private final int disposedGenerationCount;
        private final int noWritersGenerationCount;
        private final int sampleRank;
        private final int generationRank;
        private final int absoluteGenerationRank;

        private SampleImpl(SampleImpl<T> o) {
            env = o.env;
            data = o.data;
            sampleState = o.sampleState;
            viewState = o.viewState;
            instanceState = o.instanceState;
            sourceTimestamp = o.sourceTimestamp;
            instance = (HandleImpl) o.instance.modifiableCopy();
            publication = (HandleImpl) o.publication.modifiableCopy();
            disposedGenerationCount = o.disposedGenerationCount;
            noWritersGenerationCount = o.noWritersGenerationCount;
            sampleRank = o.sampleRank;
            generationRank = o.generationRank;
            absoluteGenerationRank = o.absoluteGenerationRank;
        }

        /** The sample of the rdds_sample_info at s, with data. */
        SampleImpl(ServiceEnvironment env, MemorySegment s, T data) {
            this.env = env;
            this.data = data;
            sampleState = s.get(JAVA_INT, 0) == 1 ? SampleState.READ : SampleState.NOT_READ;
            viewState = s.get(JAVA_INT, 4) == 1 ? ViewState.NEW : ViewState.NOT_NEW;
            int i = s.get(JAVA_INT, 8);
            instanceState = i == 1 ? InstanceState.ALIVE
                    : i == 2 ? InstanceState.NOT_ALIVE_DISPOSED : InstanceState.NOT_ALIVE_NO_WRITERS;
            disposedGenerationCount = s.get(JAVA_INT, 12);
            noWritersGenerationCount = s.get(JAVA_INT, 16);
            sampleRank = s.get(JAVA_INT, 20);
            generationRank = s.get(JAVA_INT, 24);
            absoluteGenerationRank = s.get(JAVA_INT, 28);
            sourceTimestamp = new TimeImpl(env, TimeImpl.get(s, 32));
            instance = HandleImpl.get(env, s, 40);
            publication = HandleImpl.get(env, s, 56);
        }

        static boolean validData(MemorySegment s) {
            return s.get(JAVA_BOOLEAN, 80);
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return env;
        }

        @Override
        public T getData() {
            return data;
        }

        @Override
        public SampleState getSampleState() {
            return sampleState;
        }

        @Override
        public ViewState getViewState() {
            return viewState;
        }

        @Override
        public InstanceState getInstanceState() {
            return instanceState;
        }

        @Override
        public Time getSourceTimestamp() {
            return sourceTimestamp;
        }

        @Override
        public ModifiableInstanceHandle getInstanceHandle() {
            return instance;
        }

        @Override
        public ModifiableInstanceHandle getPublicationHandle() {
            return publication;
        }

        @Override
        public int getDisposedGenerationCount() {
            return disposedGenerationCount;
        }

        @Override
        public int getNoWritersGenerationCount() {
            return noWritersGenerationCount;
        }

        @Override
        public int getSampleRank() {
            return sampleRank;
        }

        @Override
        public int getGenerationRank() {
            return generationRank;
        }

        @Override
        public int getAbsoluteGenerationRank() {
            return absoluteGenerationRank;
        }

        @Override
        public Sample<T> clone() {
            return new SampleImpl<>(this);
        }

        @Override
        public String toString() {
            return "Sample(" + data + ", " + sampleState + ", " + viewState + ", " + instanceState + ")";
        }
    }

    /** The samples of a read or take, copied: closing returns nothing. */
    static final class Iter<T> implements Sample.Iterator<T> {
        private final ListIterator<Sample<T>> it;

        Iter(List<Sample<T>> samples) {
            this.it = samples.listIterator();
        }

        @Override
        public void close() {
        }

        @Override
        public boolean hasNext() {
            return it.hasNext();
        }

        @Override
        public Sample<T> next() {
            if (!it.hasNext()) {
                throw new NoSuchElementException();
            }
            return it.next();
        }

        @Override
        public boolean hasPrevious() {
            return it.hasPrevious();
        }

        @Override
        public Sample<T> previous() {
            return it.previous();
        }

        @Override
        public int nextIndex() {
            return it.nextIndex();
        }

        @Override
        public int previousIndex() {
            return it.previousIndex();
        }

        @Override
        public void remove() {
            throw new UnsupportedOperationException();
        }

        @Override
        public void set(Sample<T> o) {
            throw new UnsupportedOperationException();
        }

        @Override
        public void add(Sample<T> o) {
            throw new UnsupportedOperationException();
        }
    }

    /** The selection criteria of DataReader.select(), immutable. */
    static final class Selector<T> implements DataReader.Selector<T> {
        final ReaderImpl<T> reader;
        final InstanceHandle instance;
        final boolean nextInstance;
        final DataStateImpl state;
        final int maxSamples;
        final Conditions.Read<T> condition;

        Selector(ReaderImpl<T> reader, InstanceHandle instance, boolean nextInstance, DataStateImpl state,
                int maxSamples, Conditions.Read<T> condition) {
            this.reader = reader;
            this.instance = instance;
            this.nextInstance = nextInstance;
            this.state = state;
            this.maxSamples = maxSamples;
            this.condition = condition;
        }

        @Override
        public ServiceEnvironment getEnvironment() {
            return reader.env;
        }

        @Override
        public DataReader.Selector<T> instance(InstanceHandle handle) {
            return new Selector<>(reader, handle, nextInstance, state, maxSamples, condition);
        }

        @Override
        public DataReader.Selector<T> nextInstance(boolean retrieveNextInstance) {
            return new Selector<>(reader, instance, retrieveNextInstance, state, maxSamples, condition);
        }

        @Override
        public DataReader.Selector<T> dataState(Subscriber.DataState s) {
            return new Selector<>(reader, instance, nextInstance, DataStateImpl.of(reader.env, s).clone(),
                    maxSamples, condition);
        }

        @Override
        public DataReader.Selector<T> maxSamples(int max) {
            return new Selector<>(reader, instance, nextInstance, state, max, condition);
        }

        Selector<T> condition(Conditions.Read<T> c) {
            return new Selector<>(reader, instance, nextInstance, state, maxSamples, c);
        }

        @Override
        public InstanceHandle getInstance() {
            return instance;
        }

        @Override
        public boolean retrieveNextInstance() {
            return nextInstance;
        }

        @Override
        public Subscriber.DataState getDataState() {
            return state.clone();
        }

        @Override
        public int getMaxSamples() {
            return maxSamples;
        }

        @Override
        public ReadCondition<T> getCondition() {
            return condition;
        }

        @Override
        public Sample.Iterator<T> read() {
            return new Iter<>(reader.select(this, false));
        }

        @Override
        public List<Sample<T>> read(List<Sample<T>> samples) {
            samples.clear();
            samples.addAll(reader.select(this, false));
            return samples;
        }

        @Override
        public Sample.Iterator<T> take() {
            return new Iter<>(reader.select(this, true));
        }

        @Override
        public List<Sample<T>> take(List<Sample<T>> samples) {
            samples.clear();
            samples.addAll(reader.select(this, true));
            return samples;
        }
    }
}
