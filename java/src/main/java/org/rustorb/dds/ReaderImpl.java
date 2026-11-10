// DataReader, on rdds_reader.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.policy.ResourceLimits;
import org.omg.dds.core.status.LivelinessChangedStatus;
import org.omg.dds.core.status.RequestedDeadlineMissedStatus;
import org.omg.dds.core.status.RequestedIncompatibleQosStatus;
import org.omg.dds.core.status.SampleLostStatus;
import org.omg.dds.core.status.SampleRejectedStatus;
import org.omg.dds.core.status.Status;
import org.omg.dds.core.status.SubscriptionMatchedStatus;
import org.omg.dds.sub.DataReader;
import org.omg.dds.sub.DataReaderListener;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.sub.ReadCondition;
import org.omg.dds.sub.Sample;
import org.omg.dds.sub.Subscriber;
import org.omg.dds.topic.TopicDescription;

final class ReaderImpl<T> extends EntityBase<DataReaderListener<T>, DataReaderQos> implements DataReader<T> {
    private static final int SELECT_ANY = 0;
    private static final int SELECT_INSTANCE = 1;
    private static final int SELECT_NEXT_INSTANCE = 2;

    final SubscriberImpl subscriber;
    final TopicImpl<T> topic;
    /** The content filtered topic it reads, if any. */
    final ContentFilteredTopicImpl<T> filtered;
    private final Codec<T> codec;
    private final DataReaderQos qos;
    private volatile DataReaderListener<T> listener;
    private StatusCondition<DataReader<T>> condition;
    private final List<Conditions.Read<T>> conditions = new CopyOnWriteArrayList<>();

    private ReaderImpl(SubscriberImpl subscriber, TopicImpl<T> topic, ContentFilteredTopicImpl<T> filtered,
            DataReaderQos qos, long handle, DataReaderListener<T> listener) {
        super(subscriber.env, "DataReader of " + (filtered != null ? filtered.getName() : topic.getName()), handle);
        this.subscriber = subscriber;
        this.topic = topic;
        this.filtered = filtered;
        this.codec = topic.codec();
        this.qos = qos;
        this.listener = listener;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static <T> ReaderImpl<T> create(SubscriberImpl subscriber, TopicImpl<T> topic,
            ContentFilteredTopicImpl<T> filtered, Qos.ReaderQos qos,
            DataReaderListener<T> listener, int mask) {
        Listeners.Slot slot = listener == null ? null : new Listeners.Slot(subscriber.env, listener);
        long id = slot == null ? 0 : Listeners.register(slot);
        long h;
        try {
            h = make(subscriber.env, (arena, out) -> {
                MemorySegment q = qos.toNative(arena);
                long table = slot == null ? 0 : Listeners.readerTable(arena, id).address();
                if (filtered != null) {
                    return (int) Ffi.rdds_subscriber_create_filtered_reader.invokeExact(subscriber.handle(),
                            filtered.handle(), q.address(), table, mask, out);
                }
                return (int) Ffi.rdds_subscriber_create_reader.invokeExact(subscriber.handle(), topic.handle(),
                        q.address(), table, mask, out);
            });
        } catch (RuntimeException e) {
            if (slot != null) {
                Listeners.unregister(id);
            }
            throw e;
        }
        ReaderImpl<T> r = new ReaderImpl<>(subscriber, topic, filtered, qos, h, listener);
        if (slot != null) {
            slot.ready(r);
        }
        return r;
    }

    /** The samples the selector selects, read or taken. */
    List<Sample<T>> select(Samples.Selector<T> s, boolean take) {
        if (take && s.maxSamples != ResourceLimits.LENGTH_UNLIMITED) {
            throw new UnsupportedOperationException("rustorb-dds cannot take a limited number of samples yet");
        }
        List<Sample<T>> out = new ArrayList<>();
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment p = arena.allocate(JAVA_LONG);
            int rc;
            if (s.condition != null) {
                rc = (int) Ffi.rdds_reader_select_w_condition.invokeExact(handle(), take, s.condition.handle(),
                        p.address());
            } else {
                int mode = s.nextInstance ? SELECT_NEXT_INSTANCE : s.instance != null ? SELECT_INSTANCE : SELECT_ANY;
                MemorySegment h = arena.allocate(Ffi.HANDLE);
                if (s.instance != null) {
                    HandleImpl.put(h, 0, s.instance);
                }
                rc = (int) Ffi.rdds_reader_select.invokeExact(handle(), take, s.state.sampleMask(),
                        s.state.viewMask(), s.state.instanceMask(), mode, h.address(), p.address());
            }
            Errors.check(env, rc);
            long samples = p.get(JAVA_LONG, 0);
            try {
                long n = (long) Ffi.rdds_samples_count.invokeExact(samples);
                MemorySegment info = arena.allocate(Samples.INFO_SIZE, 8);
                for (long i = 0; i < n; i++) {
                    if (!take && s.maxSamples >= 0 && out.size() >= s.maxSamples) {
                        break;
                    }
                    Errors.check(env, (int) Ffi.rdds_samples_info.invokeExact(samples, i, info.address()));
                    T data = null;
                    if (Samples.SampleImpl.validData(info)) {
                        int drc = (int) Ffi.rdds_samples_data.invokeExact(samples, i, p.address());
                        Errors.check(env, drc);
                        try (In in = new In(p.get(JAVA_LONG, 0))) {
                            data = codec.read(in);
                            in.check();
                        }
                    }
                    out.add(new Samples.SampleImpl<>(env, info, data));
                }
            } finally {
                Ffi.rdds_samples_release.invokeExact(samples);
            }
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return out;
    }

    @Override
    public Selector<T> select() {
        return new Samples.Selector<>(this, null, false, new DataStateImpl(env), ResourceLimits.LENGTH_UNLIMITED,
                null);
    }

    /** A selector of the samples of a read condition of this reader. */
    Samples.Selector<T> select(Conditions.Read<T> c) {
        if (c.getParent() != this) {
            throw Errors.precondition(env, "the condition belongs to another reader");
        }
        return ((Samples.Selector<T>) select()).condition(c);
    }

    private static <T> Samples.Selector<T> selector(Selector<T> s) {
        if (s instanceof Samples.Selector<T> i) {
            return i;
        }
        throw new IllegalArgumentException("a Selector of another DDS implementation");
    }

    @SuppressWarnings("unchecked")
    @Override
    public <OTHER> DataReader<OTHER> cast() {
        return (DataReader<OTHER>) this;
    }

    @Override
    public ReadCondition<T> createReadCondition(Subscriber.DataState states) {
        DataStateImpl d = DataStateImpl.of(env, states);
        long h = Conditions.newCondition(env, out -> (int) Ffi.rdds_reader_create_readcondition.invokeExact(
                handle(), d.sampleMask(), d.viewMask(), d.instanceMask(), out));
        Conditions.Read<T> c = new Conditions.Read<>(env, h, this, d);
        conditions.add(c);
        return c;
    }

    void forget(Conditions.Read<T> c) {
        conditions.remove(c);
    }

    @Override
    public void closeContainedEntities() {
        for (Conditions.Read<T> c : conditions) {
            c.close();
        }
    }

    @Override
    public TopicDescription<T> getTopicDescription() {
        return filtered != null ? filtered : topic;
    }

    private MemorySegment status(Arena arena, long size, WriterImpl.StatusGetter g) {
        MemorySegment s = arena.allocate(size, 4);
        call(env, () -> g.get(handle(), s.address()));
        return s;
    }

    @Override
    public SampleRejectedStatus getSampleRejectedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.SampleRejected.of(env, status(arena, Statuses.REJECTED_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_sample_rejected_status.invokeExact(r, o)));
        }
    }

    @Override
    public LivelinessChangedStatus getLivelinessChangedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.LivelinessChanged.of(env, status(arena, Statuses.LIVELINESS_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_liveliness_changed_status.invokeExact(r, o)));
        }
    }

    @Override
    public RequestedDeadlineMissedStatus getRequestedDeadlineMissedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.RequestedDeadlineMissed.of(env, status(arena, Statuses.DEADLINE_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_requested_deadline_missed_status.invokeExact(r, o)));
        }
    }

    @Override
    public RequestedIncompatibleQosStatus getRequestedIncompatibleQosStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.RequestedIncompatibleQos.of(env, status(arena, Statuses.INCOMPATIBLE_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_requested_incompatible_qos_status.invokeExact(r, o)));
        }
    }

    @Override
    public SubscriptionMatchedStatus getSubscriptionMatchedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.SubscriptionMatched.of(env, status(arena, Statuses.MATCHED_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_subscription_matched_status.invokeExact(r, o)));
        }
    }

    @Override
    public SampleLostStatus getSampleLostStatus() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment s = status(arena, Statuses.COUNT_SIZE,
                    (r, o) -> (int) Ffi.rdds_reader_sample_lost_status.invokeExact(r, o));
            return new Statuses.SampleLost(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4));
        }
    }

    @Override
    public void waitForHistoricalData(Duration maxWait) {
        handle();
    }

    @Override
    public void waitForHistoricalData(long maxWait, TimeUnit unit) {
        handle();
    }

    /** Until count writers match (rustorb extension). */
    void waitForPublications(int count, Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            Errors.checkTimeout(env, (int) Ffi.rdds_reader_wait_for_publications.invokeExact(handle(),
                    (long) count, d));
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    /** Until a sample is there (rustorb extension). */
    void waitForData(Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            Errors.checkTimeout(env, (int) Ffi.rdds_reader_wait_for_data.invokeExact(handle(), d));
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    @Override
    public Set<InstanceHandle> getMatchedPublications() {
        return WriterImpl.guids(env, (buf, cap, len) -> (int) Ffi.rdds_reader_matched_publications.invokeExact(
                handle(), buf, cap, len));
    }

    @Override
    public Sample.Iterator<T> read() {
        return select().read();
    }

    @Override
    public Sample.Iterator<T> read(Selector<T> query) {
        return selector(query).read();
    }

    @Override
    public Sample.Iterator<T> read(int maxSamples) {
        return select().maxSamples(maxSamples).read();
    }

    @Override
    public List<Sample<T>> read(List<Sample<T>> samples) {
        return select().read(samples);
    }

    @Override
    public List<Sample<T>> read(List<Sample<T>> samples, Selector<T> selector) {
        return selector(selector).read(samples);
    }

    @Override
    public Sample.Iterator<T> take() {
        return select().take();
    }

    @Override
    public Sample.Iterator<T> take(int maxSamples) {
        return select().maxSamples(maxSamples).take();
    }

    @Override
    public Sample.Iterator<T> take(Selector<T> query) {
        return selector(query).take();
    }

    @Override
    public List<Sample<T>> take(List<Sample<T>> samples) {
        return select().take(samples);
    }

    @Override
    public List<Sample<T>> take(List<Sample<T>> samples, Selector<T> query) {
        return selector(query).take(samples);
    }

    /** A new value with the key of the instance (keyHolder is not
     * changed). */
    @Override
    public T getKeyValue(T keyHolder, InstanceHandle handle) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment h = arena.allocate(Ffi.HANDLE);
            HandleImpl.put(h, 0, handle);
            MemorySegment p = arena.allocate(JAVA_LONG);
            call(env, () -> (int) Ffi.rdds_reader_get_key_value.invokeExact(handle(), h, p.address()));
            try (In in = new In(p.get(JAVA_LONG, 0))) {
                T v = codec.read(in);
                in.check();
                return v;
            }
        }
    }

    @Override
    public ModifiableInstanceHandle lookupInstance(ModifiableInstanceHandle handle, T keyHolder) {
        handle.copyFrom(lookupInstance(keyHolder));
        return handle;
    }

    @Override
    public InstanceHandle lookupInstance(T keyHolder) {
        if (keyHolder == null) {
            throw new IllegalArgumentException("a null sample");
        }
        try (Arena arena = Arena.ofConfined(); Out key = new Out()) {
            codec.write(key, keyHolder);
            MemorySegment h = arena.allocate(Ffi.HANDLE);
            call(env, () -> (int) Ffi.rdds_reader_lookup_instance.invokeExact(handle(), key.handle(), h.address()));
            return HandleImpl.get(env, h, 0);
        }
    }

    @Override
    public synchronized StatusCondition<DataReader<T>> getStatusCondition() {
        if (condition == null) {
            long h = Conditions.newCondition(env,
                    out -> (int) Ffi.rdds_reader_status_condition.invokeExact(handle(), out));
            condition = new Conditions.StatusCond<>(env, h, this);
        }
        return condition;
    }

    @Override
    public Subscriber getParent() {
        return subscriber;
    }

    @Override
    public DataReaderListener<T> getListener() {
        return listener;
    }

    @Override
    public void setListener(DataReaderListener<T> l) {
        setListener(l, null);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    @Override
    public void setListener(DataReaderListener<T> l, Collection<Class<? extends Status>> statuses) {
        Listeners.Slot slot = l == null ? null : new Listeners.Slot(env, l);
        long id = slot == null ? 0 : Listeners.register(slot);
        try (Arena arena = Arena.ofConfined()) {
            long table = slot == null ? 0 : Listeners.readerTable(arena, id).address();
            int mask = StatusKinds.mask(statuses);
            call(env, () -> (int) Ffi.rdds_reader_set_listener.invokeExact(handle(), table, mask));
        } catch (RuntimeException e) {
            if (slot != null) {
                Listeners.unregister(id);
            }
            throw e;
        }
        listener = l;
        if (slot != null) {
            slot.ready(this);
        }
    }

    @Override
    public DataReaderQos getQos() {
        return qos;
    }

    @Override
    public Set<Class<? extends Status>> getStatusChanges() {
        try {
            return StatusKinds.of((int) Ffi.rdds_reader_status_changes.invokeExact(handle()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    @Override
    public InstanceHandle getInstanceHandle() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment g = arena.allocate(Ffi.HANDLE);
            call(env, () -> (int) Ffi.rdds_reader_guid.invokeExact(handle(), g.address()));
            return HandleImpl.get(env, g, 0);
        }
    }

    @Override
    public void close() {
        closeContainedEntities();
        long h = takeHandle();
        if (h == 0) {
            return;
        }
        subscriber.readers.remove(this);
        try {
            Ffi.rdds_reader_release.invokeExact(h);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    @Override
    public String toString() {
        return "DataReader(" + topic.getName() + ")";
    }
}
