// Requests and replies over DDS (DDS-RPC 1.0, the request/reply style).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.Duration;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.topic.Topic;

/** The service side: takes requests of type TReq on a request topic and
 * answers each with replies of type TRep on a reply topic. Close it when
 * done. */
public final class Replier<TReq, TRep> implements AutoCloseable {
    /** A request taken, with what its replies name. */
    public record Request<T>(T data, SampleIdentity identity) {
    }

    private final TopicImpl<TReq> request;
    private final TopicImpl<TRep> reply;
    private volatile long handle;

    /** A replier on the topics given, RELIABLE and KEEP_ALL. */
    public Replier(Topic<TReq> requestTopic, Topic<TRep> replyTopic) {
        this(requestTopic, replyTopic, null, null, null);
    }

    /** A replier on the topics given, with the QoS given (null for
     * RELIABLE and KEEP_ALL) for its writer of replies and its reader of
     * requests, announcing {@code instanceName} (null for none). */
    public Replier(Topic<TReq> requestTopic, Topic<TRep> replyTopic, DataWriterQos writerQos,
            DataReaderQos readerQos, String instanceName) {
        this.request = (TopicImpl<TReq>) requestTopic;
        this.reply = (TopicImpl<TRep>) replyTopic;
        this.handle = EntityBase.make(request.env, (arena, out) -> (int) Ffi.rdds_replier_new.invokeExact(
                request.participant.handle(), request.handle(), reply.handle(), writerQos(arena, writerQos),
                readerQos(arena, readerQos), instanceName == null ? 0L : arena.allocateFrom(instanceName).address(),
                out));
    }

    static long writerQos(Arena arena, DataWriterQos q) {
        return q == null ? 0 : ((Qos.WriterQos) q).toNative(arena).address();
    }

    static long readerQos(Arena arena, DataReaderQos q) {
        return q == null ? 0 : ((Qos.ReaderQos) q).toNative(arena).address();
    }

    private long handle() {
        long h = handle;
        if (h == 0) {
            throw Errors.closed(request.env, "Replier");
        }
        return h;
    }

    /** Waits for a request and takes it. */
    public Request<TReq> receiveRequest(Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            MemorySegment p = arena.allocate(JAVA_LONG);
            MemorySegment id = arena.allocate(SampleIdentity.LAYOUT);
            Errors.checkTimeout(request.env, (int) Ffi.rdds_replier_receive_request.invokeExact(handle(), d,
                    p.address(), id.address()));
            try (In in = new In(p.get(JAVA_LONG, 0))) {
                return new Request<>(request.codec().read(in), SampleIdentity.of(id));
            }
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    /** Sends {@code data} to the request {@code id}. */
    public void sendReply(TRep data, SampleIdentity id) {
        try (Out out = new Out(); Arena arena = Arena.ofConfined()) {
            reply.codec().write(out, data);
            MemorySegment i = arena.allocate(SampleIdentity.LAYOUT);
            id.put(i);
            EntityBase.call(request.env, () -> (int) Ffi.rdds_replier_send_reply.invokeExact(handle(),
                    out.handle(), i.address()));
        }
    }

    @Override
    public void close() {
        long h = handle;
        handle = 0;
        if (h != 0) {
            EntityBase.call(request.env, () -> {
                Ffi.rdds_replier_release.invokeExact(h);
                return 0;
            });
        }
    }
}
