// Requests and replies over DDS (DDS-RPC 1.0, the request/reply style).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.concurrent.TimeoutException;
import org.omg.dds.core.Duration;
import org.omg.dds.pub.DataWriterQos;
import org.omg.dds.sub.DataReaderQos;
import org.omg.dds.topic.Topic;

/** The client side of a service: sends requests of type TReq on a
 * request topic and gets the replies of type TRep to them on a reply
 * topic. A reply names its request by the request's
 * {@link SampleIdentity}. Close it when done. */
public final class Requester<TReq, TRep> implements AutoCloseable {
    private final TopicImpl<TReq> request;
    private final TopicImpl<TRep> reply;
    private volatile long handle;

    /** A requester on the topics given, RELIABLE and KEEP_ALL. */
    public Requester(Topic<TReq> requestTopic, Topic<TRep> replyTopic) {
        this(requestTopic, replyTopic, null, null);
    }

    /** A requester on the topics given, with the QoS given (null for
     * RELIABLE and KEEP_ALL) for its writer of requests and its reader of
     * replies. */
    public Requester(Topic<TReq> requestTopic, Topic<TRep> replyTopic, DataWriterQos writerQos,
            DataReaderQos readerQos) {
        this.request = (TopicImpl<TReq>) requestTopic;
        this.reply = (TopicImpl<TRep>) replyTopic;
        this.handle = EntityBase.make(request.env, (arena, out) -> (int) Ffi.rdds_requester_new.invokeExact(
                request.participant.handle(), request.handle(), reply.handle(),
                Replier.writerQos(arena, writerQos), Replier.readerQos(arena, readerQos), out));
    }

    private long handle() {
        long h = handle;
        if (h == 0) {
            throw Errors.closed(request.env, "Requester");
        }
        return h;
    }

    /** Waits until a replier is matched (both of its endpoints). */
    public void waitForService(Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            Errors.checkTimeout(request.env, (int) Ffi.rdds_requester_wait_for_service.invokeExact(handle(), d));
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    /** Sends a request; returns its identity, which its replies name. */
    public SampleIdentity sendRequest(TReq data) {
        try (Out out = new Out(); Arena arena = Arena.ofConfined()) {
            request.codec().write(out, data);
            MemorySegment id = arena.allocate(SampleIdentity.LAYOUT);
            EntityBase.call(request.env, () -> (int) Ffi.rdds_requester_send_request.invokeExact(handle(),
                    out.handle(), id.address()));
            return SampleIdentity.of(id);
        }
    }

    /** Waits for a reply to the request {@code id} and takes it. */
    public TRep receiveReply(SampleIdentity id, Duration timeout) throws TimeoutException {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment i = arena.allocate(SampleIdentity.LAYOUT);
            id.put(i);
            MemorySegment d = arena.allocate(Ffi.DURATION);
            DurationImpl.put(d, DurationImpl.nanos(timeout));
            MemorySegment p = arena.allocate(java.lang.foreign.ValueLayout.JAVA_LONG);
            Errors.checkTimeout(request.env, (int) Ffi.rdds_requester_receive_reply.invokeExact(handle(),
                    i.address(), d, p.address()));
            try (In in = new In(p.get(java.lang.foreign.ValueLayout.JAVA_LONG, 0))) {
                return reply.codec().read(in);
            }
        } catch (TimeoutException | RuntimeException | Error e) {
            throw e;
        } catch (Throwable e) {
            throw Library.rethrow(e);
        }
    }

    /** Sends a request and waits for its reply. */
    public TRep request(TReq data, Duration timeout) throws TimeoutException {
        return receiveReply(sendRequest(data), timeout);
    }

    @Override
    public void close() {
        long h = handle;
        handle = 0;
        if (h != 0) {
            EntityBase.call(request.env, () -> {
                Ffi.rdds_requester_release.invokeExact(h);
                return 0;
            });
        }
    }
}
