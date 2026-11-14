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
        InstanceHandle h = HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(p, 16), 0);
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
        InstanceHandle h = HandleImpl.get(g.participant().env, Library.MEMORY.asSlice(p, 16), 0);
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
        void accept(Group g, WriterImpl<Object> w) throws Throwable;
    }

    private interface ReaderCall {
        void accept(Group g, ReaderImpl<Object> r) throws Throwable;
    }

    private static void onWriter(long self, long info, WriterCall c) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            WriterImpl<Object> w = writer(g, info);
            if (w != null) {
                c.accept(g, w);
            }
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void onReader(long self, long info, ReaderCall c) {
        try {
            Group g = GROUPS.get(self);
            if (g == null) {
                return;
            }
            ReaderImpl<Object> r = reader(g, info);
            if (r != null) {
                c.accept(g, r);
            }
        } catch (Throwable t) {
            report(t);
        }
    }

    // A listener is a PublisherListener, a SubscriberListener, a
    // DomainParticipantListener or a TopicListener; these have the same
    // methods but no common interface.

    private static void wDeadline(long self, long info, long p) {
        onWriter(self, info, (g, w) -> {
            OfferedDeadlineMissedEvent<Object> e = new Statuses.OfferedDeadlineMissedEv<>(w,
                    Statuses.OfferedDeadlineMissed.of(w.env, Library.MEMORY.asSlice(p, Statuses.DEADLINE_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onOfferedDeadlineMissed(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onOfferedDeadlineMissed(e);
            }
        });
    }

    private static void wIncompatible(long self, long info, long p) {
        onWriter(self, info, (g, w) -> {
            OfferedIncompatibleQosEvent<Object> e = new Statuses.OfferedIncompatibleQosEv<>(w,
                    Statuses.OfferedIncompatibleQos.of(w.env, Library.MEMORY.asSlice(p, Statuses.INCOMPATIBLE_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onOfferedIncompatibleQos(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onOfferedIncompatibleQos(e);
            }
        });
    }

    private static void wLivelinessLost(long self, long info, long p) {
        onWriter(self, info, (g, w) -> {
            LivelinessLostEvent<Object> e = new Statuses.LivelinessLostEv<>(w,
                    Statuses.LivelinessLost.of(w.env, Library.MEMORY.asSlice(p, Statuses.COUNT_SIZE)));
            if (g.listener() instanceof PublisherListener l) {
                l.onLivelinessLost(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onLivelinessLost(e);
            }
        });
    }

    private static void wMatched(long self, long info, long p) {
        onWriter(self, info, (g, w) -> {
            PublicationMatchedEvent<Object> e = new Statuses.PublicationMatchedEv<>(w,
                    Statuses.PublicationMatched.of(w.env, Library.MEMORY.asSlice(p, Statuses.MATCHED_SIZE)));
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
        onReader(self, info, (g, r) -> {
            RequestedDeadlineMissedEvent<Object> e = new Statuses.RequestedDeadlineMissedEv<>(r,
                    Statuses.RequestedDeadlineMissed.of(r.env, Library.MEMORY.asSlice(p, Statuses.DEADLINE_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onRequestedDeadlineMissed(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onRequestedDeadlineMissed(e);
            }
        });
    }

    private static void rIncompatible(long self, long info, long p) {
        onReader(self, info, (g, r) -> {
            RequestedIncompatibleQosEvent<Object> e = new Statuses.RequestedIncompatibleQosEv<>(r,
                    Statuses.RequestedIncompatibleQos.of(r.env, Library.MEMORY.asSlice(p, Statuses.INCOMPATIBLE_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onRequestedIncompatibleQos(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onRequestedIncompatibleQos(e);
            }
        });
    }

    private static void rRejected(long self, long info, long p) {
        onReader(self, info, (g, r) -> {
            SampleRejectedEvent<Object> e = new Statuses.SampleRejectedEv<>(r,
                    Statuses.SampleRejected.of(r.env, Library.MEMORY.asSlice(p, Statuses.REJECTED_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onSampleRejected(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onSampleRejected(e);
            }
        });
    }

    private static void rLivelinessChanged(long self, long info, long p) {
        onReader(self, info, (g, r) -> {
            LivelinessChangedEvent<Object> e = new Statuses.LivelinessChangedEv<>(r,
                    Statuses.LivelinessChanged.of(r.env, Library.MEMORY.asSlice(p, Statuses.LIVELINESS_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onLivelinessChanged(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onLivelinessChanged(e);
            }
        });
    }

    private static void rDataAvailable(long self, long info) {
        onReader(self, info, (g, r) -> {
            DataAvailableEvent<Object> e = new Statuses.DataAvailableEv<>(r, new Statuses.DataAvailable(r.env));
            if (g.listener() instanceof SubscriberListener l) {
                l.onDataAvailable(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onDataAvailable(e);
            }
        });
    }

    private static void rMatched(long self, long info, long p) {
        onReader(self, info, (g, r) -> {
            SubscriptionMatchedEvent<Object> e = new Statuses.SubscriptionMatchedEv<>(r,
                    Statuses.SubscriptionMatched.of(r.env, Library.MEMORY.asSlice(p, Statuses.MATCHED_SIZE)));
            if (g.listener() instanceof SubscriberListener l) {
                l.onSubscriptionMatched(e);
            } else if (g.listener() instanceof DomainParticipantListener l) {
                l.onSubscriptionMatched(e);
            }
        });
    }

    private static void rSampleLost(long self, long info, long p) {
        onReader(self, info, (g, r) -> {
            SampleLostEvent<Object> e = new Statuses.SampleLostEv<>(r,
                    Statuses.SampleLost.of(r.env, Library.MEMORY.asSlice(p, Statuses.COUNT_SIZE)));
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
