// DataWriter, on rdds_writer.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.Collection;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import org.omg.dds.core.Duration;
import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.ModifiableInstanceHandle;
import org.omg.dds.core.StatusCondition;
import org.omg.dds.core.Time;
import org.omg.dds.core.status.LivelinessLostStatus;
import org.omg.dds.core.status.OfferedDeadlineMissedStatus;
import org.omg.dds.core.status.OfferedIncompatibleQosStatus;
import org.omg.dds.core.status.PublicationMatchedStatus;
import org.omg.dds.core.status.Status;
import org.omg.dds.pub.DataWriter;
import org.omg.dds.pub.DataWriterListener;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.pub.Publisher;
import org.omg.dds.topic.Topic;

final class WriterImpl<T> extends EntityBase<DataWriterListener<T>, DataWriterQos> implements DataWriter<T> {
    final PublisherImpl publisher;
    final TopicImpl<T> topic;
    private final Codec<T> codec;
    private final DataWriterQos qos;
    private volatile DataWriterListener<T> listener;
    private StatusCondition<DataWriter<T>> condition;
    // The instances registered or looked up, by handle, with a sample of
    // their key: the C ABI names an instance by its key.
    private final Map<InstanceHandle, Out> instances = new HashMap<>();

    private WriterImpl(PublisherImpl publisher, TopicImpl<T> topic, DataWriterQos qos, long handle,
            DataWriterListener<T> listener) {
        super(publisher.env, "DataWriter of " + topic.getName(), handle);
        this.publisher = publisher;
        this.topic = topic;
        this.codec = topic.codec();
        this.qos = qos;
        this.listener = listener;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static <T> WriterImpl<T> create(PublisherImpl publisher, TopicImpl<T> topic, Qos.WriterQos qos,
            DataWriterListener<T> listener, int mask) {
        Listeners.Slot slot = listener == null ? null : new Listeners.Slot(publisher.env, listener);
        long id = slot == null ? 0 : Listeners.register(slot);
        long h;
        try {
            h = make(publisher.env, (arena, out) -> {
                MemorySegment q = qos.toNative(arena);
                long table = slot == null ? 0 : Listeners.writerTable(arena, id).address();
                return (int) Ffi.rdds_publisher_create_writer.invokeExact(publisher.handle(), topic.handle(),
                        q.address(), table, mask, out);
            });
        } catch (RuntimeException e) {
            if (slot != null) {
                Listeners.unregister(id);
            }
            throw e;
        }
        WriterImpl<T> w = new WriterImpl<>(publisher, topic, qos, h, listener);
        if (slot != null) {
            slot.ready(w);
        }
        return w;
    }

    /** The values of a sample; IllegalArgumentException for null. */
    private Out sample(T data) {
        if (data == null) {
            throw new IllegalArgumentException("a null sample");
        }
        Out out = new Out();
        try {
            codec.write(out, data);
        } catch (RuntimeException e) {
            out.close();
            throw e;
        }
        return out;
    }

    /** An rdds_time of t, or 0 (now) for null. */
    private static long time(Arena arena, Time t) {
        if (t == null) {
            return 0;
        }
        MemorySegment s = arena.allocate(Ffi.DURATION);
        TimeImpl.put(s, TimeImpl.nanos(t));
        return s.address();
    }

    private Time timeOf(long t, TimeUnit unit) {
        return new TimeImpl(env, unit.toNanos(t));
    }

    private interface Op {
        int call(long writer, long sample, long time) throws Throwable;
    }

    private void op(Out sample, Time t, Op op) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            int rc = op.call(handle(), sample.handle(), time(arena, t));
            Errors.checkTimeout(env, rc);
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    /** The key sample of an instance this writer knows. */
    private Out known(InstanceHandle h) {
        synchronized (instances) {
            Out o = instances.get(h);
            if (o == null) {
                throw new IllegalArgumentException("rustorb-dds knows the instance " + h
                        + " only if this writer's registerInstance or lookupInstance returned it: pass the sample");
            }
            return o;
        }
    }

    private void remember(InstanceHandle h, Out key) {
        synchronized (instances) {
            if (!h.isNil() && !instances.containsKey(h) && !isClosed()) {
                instances.put(h, key);
                return;
            }
        }
        key.close();
    }

    @SuppressWarnings("unchecked")
    @Override
    public <OTHER> DataWriter<OTHER> cast() {
        return (DataWriter<OTHER>) this;
    }

    @Override
    public Topic<T> getTopic() {
        return topic;
    }

    @Override
    public void waitForAcknowledgments(Duration maxWait) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(maxWait));
            Errors.checkTimeout(env, (int) Ffi.rdds_writer_wait_for_acknowledgments.invokeExact(handle(), d));
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    @Override
    public void waitForAcknowledgments(long maxWait, TimeUnit unit) throws TimeoutException {
        waitForAcknowledgments(DurationImpl.of(env, maxWait, unit));
    }

    /** Until count readers match (rustorb extension). */
    void waitForSubscriptions(int count, Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            Errors.checkTimeout(env, (int) Ffi.rdds_writer_wait_for_subscriptions.invokeExact(handle(),
                    (long) count, d));
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    private MemorySegment status(Arena arena, long size, StatusGetter g) {
        MemorySegment s = arena.allocate(size, 4);
        call(env, () -> g.get(handle(), s.address()));
        return s;
    }

    interface StatusGetter {
        int get(long entity, long out) throws Throwable;
    }

    @Override
    public LivelinessLostStatus getLivelinessLostStatus() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment s = status(arena, Statuses.COUNT_SIZE,
                    (w, o) -> (int) Ffi.rdds_writer_liveliness_lost_status.invokeExact(w, o));
            return new Statuses.LivelinessLost(env, s.get(JAVA_INT, 0), s.get(JAVA_INT, 4));
        }
    }

    @Override
    public OfferedDeadlineMissedStatus getOfferedDeadlineMissedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.OfferedDeadlineMissed.of(env, status(arena, Statuses.DEADLINE_SIZE,
                    (w, o) -> (int) Ffi.rdds_writer_offered_deadline_missed_status.invokeExact(w, o)));
        }
    }

    @Override
    public OfferedIncompatibleQosStatus getOfferedIncompatibleQosStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.OfferedIncompatibleQos.of(env, status(arena, Statuses.INCOMPATIBLE_SIZE,
                    (w, o) -> (int) Ffi.rdds_writer_offered_incompatible_qos_status.invokeExact(w, o)));
        }
    }

    @Override
    public PublicationMatchedStatus getPublicationMatchedStatus() {
        try (Arena arena = Arena.ofConfined()) {
            return Statuses.PublicationMatched.of(env, status(arena, Statuses.MATCHED_SIZE,
                    (w, o) -> (int) Ffi.rdds_writer_publication_matched_status.invokeExact(w, o)));
        }
    }

    @Override
    public void assertLiveliness() {
        call(env, () -> (int) Ffi.rdds_writer_assert_liveliness.invokeExact(handle()));
    }

    @Override
    public Set<InstanceHandle> getMatchedSubscriptions() {
        return guids(env, (buf, cap, len) -> (int) Ffi.rdds_writer_matched_subscriptions.invokeExact(handle(), buf,
                cap, len));
    }

    interface GuidLister {
        int list(long buf, long cap, long len) throws Throwable;
    }

    /** The GUIDs a C function lists, as handles. */
    static Set<InstanceHandle> guids(org.omg.dds.core.ServiceEnvironment env, GuidLister f) {
        long cap = 16;
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment len = arena.allocate(JAVA_LONG);
            while (true) {
                MemorySegment buf = arena.allocate(16 * cap, 1);
                long c = cap;
                call(env, () -> f.list(buf.address(), c, len.address()));
                long n = len.get(JAVA_LONG, 0);
                if (n <= cap) {
                    Set<InstanceHandle> s = new LinkedHashSet<>();
                    for (long i = 0; i < n; i++) {
                        s.add(HandleImpl.get(env, buf, 16 * i));
                    }
                    return Collections.unmodifiableSet(s);
                }
                cap = n;
            }
        }
    }

    @Override
    public InstanceHandle registerInstance(T instanceData) throws TimeoutException {
        return registerInstance(instanceData, null);
    }

    @Override
    public InstanceHandle registerInstance(T instanceData, Time sourceTimestamp) throws TimeoutException {
        Out key = sample(instanceData);
        try (Arena arena = Arena.ofConfined()) {
            // rdds_writer_register_instance takes no timestamp.
            MemorySegment h = arena.allocate(Ffi.HANDLE);
            int rc = (int) Ffi.rdds_writer_register_instance.invokeExact(handle(), key.handle(), h.address());
            Errors.checkTimeout(env, rc);
            HandleImpl handle = HandleImpl.get(env, h, 0);
            remember(handle, key);
            return handle;
        } catch (TimeoutException | RuntimeException | Error e) {
            key.close();
            throw e;
        } catch (Throwable e) {
            key.close();
            throw Library.rethrow(e);
        }
    }

    @Override
    public InstanceHandle registerInstance(T instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException {
        return registerInstance(instanceData, timeOf(sourceTimestamp, unit));
    }

    @Override
    public void unregisterInstance(InstanceHandle handle) throws TimeoutException {
        unregisterInstance(handle, null, null);
    }

    @Override
    public void unregisterInstance(InstanceHandle handle, T instanceData) throws TimeoutException {
        unregisterInstance(handle, instanceData, null);
    }

    @Override
    public void unregisterInstance(InstanceHandle handle, T instanceData, Time sourceTimestamp)
            throws TimeoutException {
        Op f = (w, s, t) -> (int) Ffi.rdds_writer_unregister_instance.invokeExact(w, s, t);
        if (instanceData == null) {
            op(known(handle), sourceTimestamp, f);
        } else {
            try (Out s = sample(instanceData)) {
                op(s, sourceTimestamp, f);
            }
        }
        if (handle != null && !handle.isNil()) {
            Out o;
            synchronized (instances) {
                o = instances.remove(handle);
            }
            if (o != null) {
                o.close();
            }
        }
    }

    @Override
    public void unregisterInstance(InstanceHandle handle, T instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException {
        unregisterInstance(handle, instanceData, timeOf(sourceTimestamp, unit));
    }

    @Override
    public void write(T instanceData) throws TimeoutException {
        write(instanceData, null, null);
    }

    @Override
    public void write(T instanceData, Time sourceTimestamp) throws TimeoutException {
        write(instanceData, null, sourceTimestamp);
    }

    @Override
    public void write(T instanceData, long sourceTimestamp, TimeUnit unit) throws TimeoutException {
        write(instanceData, null, timeOf(sourceTimestamp, unit));
    }

    @Override
    public void write(T instanceData, InstanceHandle handle) throws TimeoutException {
        write(instanceData, handle, null);
    }

    /** The handle is not needed: rustorb-dds finds the instance by the
     * key of the sample. */
    @Override
    public void write(T instanceData, InstanceHandle handle, Time sourceTimestamp) throws TimeoutException {
        try (Out s = sample(instanceData)) {
            op(s, sourceTimestamp, (w, d, t) -> (int) Ffi.rdds_writer_write.invokeExact(w, d, t));
        }
    }

    @Override
    public void write(T instanceData, InstanceHandle handle, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException {
        write(instanceData, handle, timeOf(sourceTimestamp, unit));
    }

    @Override
    public void dispose(InstanceHandle instanceHandle) throws TimeoutException {
        dispose(instanceHandle, null, null);
    }

    @Override
    public void dispose(InstanceHandle instanceHandle, T instanceData) throws TimeoutException {
        dispose(instanceHandle, instanceData, null);
    }

    @Override
    public void dispose(InstanceHandle instanceHandle, T instanceData, Time sourceTimestamp)
            throws TimeoutException {
        Op f = (w, s, t) -> (int) Ffi.rdds_writer_dispose.invokeExact(w, s, t);
        if (instanceData == null) {
            op(known(instanceHandle), sourceTimestamp, f);
        } else {
            try (Out s = sample(instanceData)) {
                op(s, sourceTimestamp, f);
            }
        }
    }

    @Override
    public void dispose(InstanceHandle instanceHandle, T instanceData, long sourceTimestamp, TimeUnit unit)
            throws TimeoutException {
        dispose(instanceHandle, instanceData, timeOf(sourceTimestamp, unit));
    }

    /** A new value with the key of the instance (keyHolder is not
     * changed). */
    @Override
    public T getKeyValue(T keyHolder, InstanceHandle handle) {
        return getKeyValue(handle);
    }

    @Override
    public T getKeyValue(InstanceHandle handle) {
        Out key;
        synchronized (instances) {
            key = known(handle);
            TypeRef t = codec.getType();
            byte[] data = t.serialize(key, false);
            try (In in = t.deserialize(data)) {
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
        Out key = sample(keyHolder);
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment h = arena.allocate(Ffi.HANDLE);
            call(env, () -> (int) Ffi.rdds_writer_lookup_instance.invokeExact(handle(), key.handle(), h.address()));
            HandleImpl handle = HandleImpl.get(env, h, 0);
            remember(handle, key);
            return handle;
        } catch (RuntimeException e) {
            key.close();
            throw e;
        }
    }

    @Override
    public synchronized StatusCondition<DataWriter<T>> getStatusCondition() {
        if (condition == null) {
            long h = Conditions.newCondition(env,
                    out -> (int) Ffi.rdds_writer_status_condition.invokeExact(handle(), out));
            condition = new Conditions.StatusCond<>(env, h, this);
        }
        return condition;
    }

    @Override
    public Publisher getParent() {
        return publisher;
    }

    @Override
    public DataWriterListener<T> getListener() {
        return listener;
    }

    @Override
    public void setListener(DataWriterListener<T> l) {
        setListener(l, null);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    @Override
    public void setListener(DataWriterListener<T> l, Collection<Class<? extends Status>> statuses) {
        Listeners.Slot slot = l == null ? null : new Listeners.Slot(env, l);
        long id = slot == null ? 0 : Listeners.register(slot);
        try (Arena arena = Arena.ofConfined()) {
            long table = slot == null ? 0 : Listeners.writerTable(arena, id).address();
            int mask = StatusKinds.mask(statuses);
            call(env, () -> (int) Ffi.rdds_writer_set_listener.invokeExact(handle(), table, mask));
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
    public DataWriterQos getQos() {
        return qos;
    }

    @Override
    public Set<Class<? extends Status>> getStatusChanges() {
        try {
            return StatusKinds.of((int) Ffi.rdds_writer_status_changes.invokeExact(handle()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    @Override
    public InstanceHandle getInstanceHandle() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment g = arena.allocate(Ffi.HANDLE);
            call(env, () -> (int) Ffi.rdds_writer_guid.invokeExact(handle(), g.address()));
            return HandleImpl.get(env, g, 0);
        }
    }

    @Override
    public void close() {
        long h = takeHandle();
        if (h == 0) {
            return;
        }
        publisher.writers.remove(this);
        try {
            Ffi.rdds_writer_release.invokeExact(h);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        synchronized (instances) {
            for (Out o : instances.values()) {
                o.close();
            }
            instances.clear();
        }
    }

    @Override
    public String toString() {
        return "DataWriter(" + topic.getName() + ")";
    }
}
