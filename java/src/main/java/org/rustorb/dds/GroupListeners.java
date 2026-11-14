// The listeners of participants, publishers, subscribers and topics: an
// rdds_listener table of upcall stubs whose self is the number of a
// Group. The event of a writer or reader has as source the Java object of
// that writer or reader, found by its GUID among the participant's; a
// status of an endpoint this binding did not make is not passed on.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandles;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

import org.omg.dds.core.InstanceHandle;
import org.omg.dds.core.event.DataAvailableEvent;
import org.omg.dds.core.event.DataOnReadersEvent;
import org.omg.dds.core.event.LivelinessChangedEvent;
import org.omg.dds.core.event.LivelinessLostEvent;
import org.omg.dds.core.event.OfferedDeadlineMissedEvent;
import org.omg.dds.core.event.OfferedIncompatibleQosEvent;
import org.omg.dds.core.event.PublicationMatchedEvent;
import org.omg.dds.core.event.RequestedDeadlineMissedEvent;
import org.omg.dds.core.event.RequestedIncompatibleQosEvent;
import org.omg.dds.core.event.SampleLostEvent;
import org.omg.dds.core.event.SampleRejectedEvent;
import org.omg.dds.core.event.SubscriptionMatchedEvent;
import org.omg.dds.domain.DomainParticipantListener;
import org.omg.dds.pub.PublisherListener;
import org.omg.dds.sub.SubscriberListener;
import org.omg.dds.topic.TopicListener;

final class GroupListeners {
    private GroupListeners() {}

    /** A listener set on a participant, publisher, subscriber or topic. */
    record Group(ParticipantImpl participant, Object listener, TopicImpl<?> topic) {}

    private static final ConcurrentHashMap<Long, Group> GROUPS = new ConcurrentHashMap<>();
    private static final AtomicLong NEXT = new AtomicLong(1);

    private static void report(Throwable t) {
        System.err.println("rustorb-dds: a listener threw:");
        t.printStackTrace();
    }

    private static final FunctionDescriptor ENDPOINT_STATUS = FunctionDescriptor.ofVoid(JAVA_LONG, JAVA_LONG, JAVA_LONG);
    private static final FunctionDescriptor TWO = FunctionDescriptor.ofVoid(JAVA_LONG, JAVA_LONG);
    private static final FunctionDescriptor SELF = FunctionDescriptor.ofVoid(JAVA_LONG);
    private static final MethodHandles.Lookup LOOKUP = MethodHandles.lookup();
    private static final long WDEADLINE = Library.upcall(LOOKUP, "wDeadline", ENDPOINT_STATUS);
    private static final long WINCOMPATIBLE = Library.upcall(LOOKUP, "wIncompatible", ENDPOINT_STATUS);
    private static final long WLIVELINESSLOST = Library.upcall(LOOKUP, "wLivelinessLost", ENDPOINT_STATUS);
    private static final long WMATCHED = Library.upcall(LOOKUP, "wMatched", ENDPOINT_STATUS);
    private static final long DATAONREADERS = Library.upcall(LOOKUP, "dataOnReaders", TWO);
    private static final long RDEADLINE = Library.upcall(LOOKUP, "rDeadline", ENDPOINT_STATUS);
    private static final long RINCOMPATIBLE = Library.upcall(LOOKUP, "rIncompatible", ENDPOINT_STATUS);
    private static final long RREJECTED = Library.upcall(LOOKUP, "rRejected", ENDPOINT_STATUS);
    private static final long RLIVELINESSCHANGED = Library.upcall(LOOKUP, "rLivelinessChanged", ENDPOINT_STATUS);
    private static final long RDATAAVAILABLE = Library.upcall(LOOKUP, "rDataAvailable", TWO);
    private static final long RMATCHED = Library.upcall(LOOKUP, "rMatched", ENDPOINT_STATUS);
    private static final long RSAMPLELOST = Library.upcall(LOOKUP, "rSampleLost", ENDPOINT_STATUS);
    private static final long INCONSISTENT = Library.upcall(LOOKUP, "inconsistent", ENDPOINT_STATUS);
    private static final long RELEASE = Library.upcall(LOOKUP, "release", SELF);

    /** The writer or reader whose rdds_endpoint_info is at p, or null. */
    private static WriterImpl<Object> writer(Group g, long p) {
        return writer(g, HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(p, 16), 0));
    }

    private static WriterImpl<Object> writer(Group g, InstanceHandle h) {
        for (PublisherImpl pub : g.participant().publishers) {
            for (WriterImpl<?> w : pub.writers) {
                if (h.equals(w.getInstanceHandle())) {
                    @SuppressWarnings("unchecked")
                    WriterImpl<Object> o = (WriterImpl<Object>) w;
                    return o;
                }
            }
        }
        return null;
    }

    private static ReaderImpl<Object> reader(Group g, long p) {
        return reader(g, HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(p, 16), 0));
    }

    private static ReaderImpl<Object> reader(Group g, InstanceHandle h) {
        for (SubscriberImpl sub : g.participant().subscribers) {
            for (ReaderImpl<?> r : sub.readers) {
                if (h.equals(r.getInstanceHandle())) {
                    @SuppressWarnings("unchecked")
                    ReaderImpl<Object> o = (ReaderImpl<Object>) r;
                    return o;
                }
            }
        }
        return null;
    }

    private interface WriterCall {
        void accept(Group g, WriterImpl<Object> w, long status) throws Throwable;
    }

    private interface ReaderCall {
        void accept(Group g, ReaderImpl<Object> r, long status) throws Throwable;
    }

    // A status of a writer or reader still being made (one matched in its
    // own participant while created) waits here, by its handle, until
    // created() is called for it; the status is copied.
    private static final Object LOCK = new Object();
    private static final HashMap<InstanceHandle, List<Runnable>> PENDING = new HashMap<>();

    /** Passes on the statuses that came while the writer or reader of handle was being made. */
    static void created(InstanceHandle handle) {
        List<Runnable> calls;
        synchronized (LOCK) {
            calls = PENDING.remove(handle);
        }
        if (calls != null) {
            calls.forEach(Runnable::run);
        }
    }

    private static MemorySegment copy(long p, long size) {
        MemorySegment c = Arena.ofAuto().allocate(Math.max(size, 1), 8);
        if (size > 0) {
            c.copyFrom(Library.MEMORY.asSlice(p, size));
        }
        return c;
    }

    private static void onWriter(long self, long info, long p, long size, WriterCall c) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            WriterImpl<Object> w;
            synchronized (LOCK) {
                w = writer(g, info);
                if (w == null) {
                    InstanceHandle h = HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(info, 16), 0);
                    MemorySegment status = copy(p, size);
                    PENDING.computeIfAbsent(h, k -> new ArrayList<>()).add(() -> {
                        try {
                            WriterImpl<Object> v = writer(g, h);
                            if (v != null && GROUPS.containsKey(self)) {
                                c.accept(g, v, status.address());
                            }
                        } catch (Throwable t) {
                            report(t);
                        }
                    });
                    return;
                }
            }
            c.accept(g, w, p);
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void onReader(long self, long info, long p, long size, ReaderCall c) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            ReaderImpl<Object> r;
            synchronized (LOCK) {
                r = reader(g, info);
                if (r == null) {
                    InstanceHandle h = HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(info, 16), 0);
                    MemorySegment status = copy(p, size);
                    PENDING.computeIfAbsent(h, k -> new ArrayList<>()).add(() -> {
                        try {
                            ReaderImpl<Object> v = reader(g, h);
                            if (v != null && GROUPS.containsKey(self)) {
                                c.accept(g, v, status.address());
                            }
                        } catch (Throwable t) {
                            report(t);
                        }
                    });
                    return;
                }
            }
            c.accept(g, r, p);
        } catch (Throwable t) {
            report(t);
        }
    }

    // A listener is a PublisherListener, a SubscriberListener, a
    // DomainParticipantListener or a TopicListener; these have the same
    // methods but no common interface.

    private static void wDeadline(long self, long info, long p) {
        onWriter(self, info, p, Statuses.DEADLINE_SIZE, (g, w, q) -> {
            OfferedDeadlineMissedEvent<Object> e = new Statuses.OfferedDeadlineMissedEv<>(w,
                    Statuses.OfferedDeadlineMissed.of(w.env, Library.MEMORY.asSlice(q, Statuses.DEADLINE_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onOfferedDeadlineMissed(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onOfferedDeadlineMissed(e);
            }
        });
    }

    private static void wIncompatible(long self, long info, long p) {
        onWriter(self, info, p, Statuses.INCOMPATIBLE_SIZE, (g, w, q) -> {
            OfferedIncompatibleQosEvent<Object> e = new Statuses.OfferedIncompatibleQosEv<>(w,
                    Statuses.OfferedIncompatibleQos.of(w.env, Library.MEMORY.asSlice(q, Statuses.INCOMPATIBLE_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onOfferedIncompatibleQos(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onOfferedIncompatibleQos(e);
            }
        });
    }

    private static void wLivelinessLost(long self, long info, long p) {
        onWriter(self, info, p, Statuses.COUNT_SIZE, (g, w, q) -> {
            LivelinessLostEvent<Object> e = new Statuses.LivelinessLostEv<>(w,
                    Statuses.LivelinessLost.of(w.env, Library.MEMORY.asSlice(q, Statuses.COUNT_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onLivelinessLost(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onLivelinessLost(e);
            }
        });
    }

    private static void wMatched(long self, long info, long p) {
        onWriter(self, info, p, Statuses.MATCHED_SIZE, (g, w, q) -> {
            PublicationMatchedEvent<Object> e = new Statuses.PublicationMatchedEv<>(w,
                    Statuses.PublicationMatched.of(w.env, Library.MEMORY.asSlice(q, Statuses.MATCHED_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onPublicationMatched(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onPublicationMatched(e);
            }
        });
    }

    private static void dataOnReaders(long self, long subscriber) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            for (SubscriberImpl s : g.participant().subscribers) {
                boolean same = (boolean) Ffi.rdds_subscriber_same.invokeExact(s.handle(), subscriber);
                if (!same) {
                    continue;
                }
                DataOnReadersEvent e = new Statuses.DataOnReadersEv(s, new Statuses.DataOnReaders(s.env));
                if (g.listener() instanceof SubscriberListener l) {
                    l.onDataOnReaders(e);
                } else if (g.listener() instanceof DomainParticipantListener l) {
                    l.onDataOnReaders(e);
                }
                return;
            }
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rDeadline(long self, long info, long p) {
        onReader(self, info, p, Statuses.DEADLINE_SIZE, (g, r, q) -> {
            RequestedDeadlineMissedEvent<Object> e = new Statuses.RequestedDeadlineMissedEv<>(r,
                    Statuses.RequestedDeadlineMissed.of(r.env, Library.MEMORY.asSlice(q, Statuses.DEADLINE_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onRequestedDeadlineMissed(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onRequestedDeadlineMissed(e);
            }
        });
    }

    private static void rIncompatible(long self, long info, long p) {
        onReader(self, info, p, Statuses.INCOMPATIBLE_SIZE, (g, r, q) -> {
            RequestedIncompatibleQosEvent<Object> e = new Statuses.RequestedIncompatibleQosEv<>(r,
                    Statuses.RequestedIncompatibleQos.of(r.env, Library.MEMORY.asSlice(q, Statuses.INCOMPATIBLE_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onRequestedIncompatibleQos(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onRequestedIncompatibleQos(e);
            }
        });
    }

    private static void rRejected(long self, long info, long p) {
        onReader(self, info, p, Statuses.REJECTED_SIZE, (g, r, q) -> {
            SampleRejectedEvent<Object> e = new Statuses.SampleRejectedEv<>(r,
                    Statuses.SampleRejected.of(r.env, Library.MEMORY.asSlice(q, Statuses.REJECTED_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onSampleRejected(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onSampleRejected(e);
            }
        });
    }

    private static void rLivelinessChanged(long self, long info, long p) {
        onReader(self, info, p, Statuses.LIVELINESS_SIZE, (g, r, q) -> {
            LivelinessChangedEvent<Object> e = new Statuses.LivelinessChangedEv<>(r,
                    Statuses.LivelinessChanged.of(r.env, Library.MEMORY.asSlice(q, Statuses.LIVELINESS_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onLivelinessChanged(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onLivelinessChanged(e);
            }
        });
    }

    private static void rDataAvailable(long self, long info) {
        onReader(self, info, 0, 0, (g, r, q) -> {
            DataAvailableEvent<Object> e = new Statuses.DataAvailableEv<>(r, new Statuses.DataAvailable(r.env));
            if (g.listener() instanceof SubscriberListener l) {
                l.onDataAvailable(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onDataAvailable(e);
            }
        });
    }

    private static void rMatched(long self, long info, long p) {
        onReader(self, info, p, Statuses.MATCHED_SIZE, (g, r, q) -> {
            SubscriptionMatchedEvent<Object> e = new Statuses.SubscriptionMatchedEv<>(r,
                    Statuses.SubscriptionMatched.of(r.env, Library.MEMORY.asSlice(q, Statuses.MATCHED_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onSubscriptionMatched(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onSubscriptionMatched(e);
            }
        });
    }

    private static void rSampleLost(long self, long info, long p) {
        onReader(self, info, p, Statuses.COUNT_SIZE, (g, r, q) -> {
            SampleLostEvent<Object> e = new Statuses.SampleLostEv<>(r,
                    Statuses.SampleLost.of(r.env, Library.MEMORY.asSlice(q, Statuses.COUNT_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onSampleLost(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onSampleLost(e);
            }
        });
    }

    @SuppressWarnings("unchecked")
    private static void inconsistent(long self, long name, long p) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            TopicImpl<Object> topic = (TopicImpl<Object>) g.topic();
            if (topic == null) {
                String n = Library.cString(name);
                for (TopicImpl<?> t : g.participant().topics) {
                    if (t.getName().equals(n)) {
                        topic = (TopicImpl<Object>) t;
                        break;
                    }
                }
            }
            if (topic == null) {
                return;
            }
            Statuses.InconsistentTopicEv<Object> e = new Statuses.InconsistentTopicEv<>(topic,
                    Statuses.InconsistentTopic.of(topic.env, Library.MEMORY.asSlice(p, Statuses.COUNT_SIZE)));
            if (g.listener() instanceof TopicListener<?> l) {
                ((TopicListener<Object>) l).onInconsistentTopic(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onInconsistentTopic(e);
            }
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void release(long self) {
        GROUPS.remove(self);
    }

    interface Setter {
        int set(long table, int mask) throws Throwable;
    }

    /** Sets the listener of an entity with set (null clears it). */
    static void set(ParticipantImpl participant, Object listener, TopicImpl<?> topic, int mask, Setter set) {
        if (listener == null) {
            EntityBase.call(participant.env, () -> set.set(0, 0));
            return;
        }
        long id = NEXT.getAndIncrement();
        GROUPS.put(id, new Group(participant, listener, topic));
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment t = table(arena, id);
            EntityBase.call(participant.env, () -> set.set(t.address(), mask));
        } catch (RuntimeException e) {
            GROUPS.remove(id);
            throw e;
        }
    }

    /** An rdds_listener calling the group numbered id. */
    private static MemorySegment table(Arena arena, long id) {
        long[] fns = {id, WDEADLINE, WINCOMPATIBLE, WLIVELINESSLOST, WMATCHED, DATAONREADERS, RDEADLINE,
            RINCOMPATIBLE, RREJECTED, RLIVELINESSCHANGED, RDATAAVAILABLE, RMATCHED, RSAMPLELOST, INCONSISTENT,
            RELEASE};
        MemorySegment t = arena.allocate(fns.length * 8L, 8);
        for (int i = 0; i < fns.length; i++) {
            t.set(JAVA_LONG, i * 8L, fns[i]);
        }
        return t;
    }
}
