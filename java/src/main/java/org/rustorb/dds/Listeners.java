// The listeners of writers and readers: function tables of upcall stubs
// (rdds_writer_listener, rdds_reader_listener) whose self is the number
// of a Slot.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandles;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

import org.omg.dds.core.ServiceEnvironment;
import org.omg.dds.pub.DataWriterListener;
import org.omg.dds.sub.DataReaderListener;

final class Listeners {
    private Listeners() {}

    /**
     * A listener set on an entity. The library may call it while the
     * entity is being made, before the Java object exists: those calls
     * wait in pending until {@link #ready}.
     */
    static final class Slot<E, L> {
        final ServiceEnvironment env;
        final L listener;
        private E entity;
        private List<Consumer<E>> pending = new ArrayList<>();

        Slot(ServiceEnvironment env, L listener) {
            this.env = env;
            this.listener = listener;
        }

        void post(Consumer<E> call) {
            E e;
            synchronized (this) {
                if (pending != null) {
                    pending.add(call);
                    return;
                }
                e = entity;
            }
            call.accept(e);
        }

        void ready(E e) {
            List<Consumer<E>> calls;
            synchronized (this) {
                entity = e;
                calls = pending;
                pending = null;
            }
            for (Consumer<E> c : calls) {
                c.accept(e);
            }
        }
    }

    private static final ConcurrentHashMap<Long, Slot<?, ?>> SLOTS = new ConcurrentHashMap<>();
    private static final AtomicLong NEXT = new AtomicLong(1);

    static long register(Slot<?, ?> slot) {
        long id = NEXT.getAndIncrement();
        SLOTS.put(id, slot);
        return id;
    }

    static void unregister(long id) {
        SLOTS.remove(id);
    }

    @SuppressWarnings("unchecked")
    private static <E, L> Slot<E, L> slot(long self) {
        return (Slot<E, L>) SLOTS.get(self);
    }

    private static void report(Throwable t) {
        System.err.println("rustorb-dds: a listener threw:");
        t.printStackTrace();
    }

    private static final FunctionDescriptor STATUS = FunctionDescriptor.ofVoid(JAVA_LONG, JAVA_LONG);
    private static final FunctionDescriptor SELF = FunctionDescriptor.ofVoid(JAVA_LONG);
    private static final MethodHandles.Lookup LOOKUP = MethodHandles.lookup();
    private static final long WDEADLINE = Library.upcall(LOOKUP, "wDeadline", STATUS);
    private static final long WINCOMPATIBLE = Library.upcall(LOOKUP, "wIncompatible", STATUS);
    private static final long WLIVELINESSLOST = Library.upcall(LOOKUP, "wLivelinessLost", STATUS);
    private static final long WMATCHED = Library.upcall(LOOKUP, "wMatched", STATUS);
    private static final long RDEADLINE = Library.upcall(LOOKUP, "rDeadline", STATUS);
    private static final long RINCOMPATIBLE = Library.upcall(LOOKUP, "rIncompatible", STATUS);
    private static final long RREJECTED = Library.upcall(LOOKUP, "rRejected", STATUS);
    private static final long RLIVELINESSCHANGED = Library.upcall(LOOKUP, "rLivelinessChanged", STATUS);
    private static final long RDATAAVAILABLE = Library.upcall(LOOKUP, "rDataAvailable", SELF);
    private static final long RMATCHED = Library.upcall(LOOKUP, "rMatched", STATUS);
    private static final long RSAMPLELOST = Library.upcall(LOOKUP, "rSampleLost", STATUS);
    private static final long RELEASE = Library.upcall(LOOKUP, "release", SELF);

    private static void wDeadline(long self, long p) {
        try {
            Slot<WriterImpl<Object>, DataWriterListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.OfferedDeadlineMissed status = Statuses.OfferedDeadlineMissed.of(s.env, Library.MEMORY.asSlice(p, Statuses.DEADLINE_SIZE));
            s.post(e -> {
                try {
                    s.listener.onOfferedDeadlineMissed(new Statuses.OfferedDeadlineMissedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void wIncompatible(long self, long p) {
        try {
            Slot<WriterImpl<Object>, DataWriterListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.OfferedIncompatibleQos status = Statuses.OfferedIncompatibleQos.of(s.env, Library.MEMORY.asSlice(p, Statuses.INCOMPATIBLE_SIZE));
            s.post(e -> {
                try {
                    s.listener.onOfferedIncompatibleQos(new Statuses.OfferedIncompatibleQosEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void wLivelinessLost(long self, long p) {
        try {
            Slot<WriterImpl<Object>, DataWriterListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.LivelinessLost status = Statuses.LivelinessLost.of(s.env, Library.MEMORY.asSlice(p, Statuses.COUNT_SIZE));
            s.post(e -> {
                try {
                    s.listener.onLivelinessLost(new Statuses.LivelinessLostEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void wMatched(long self, long p) {
        try {
            Slot<WriterImpl<Object>, DataWriterListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.PublicationMatched status = Statuses.PublicationMatched.of(s.env, Library.MEMORY.asSlice(p, Statuses.MATCHED_SIZE));
            s.post(e -> {
                try {
                    s.listener.onPublicationMatched(new Statuses.PublicationMatchedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rDeadline(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.RequestedDeadlineMissed status = Statuses.RequestedDeadlineMissed.of(s.env, Library.MEMORY.asSlice(p, Statuses.DEADLINE_SIZE));
            s.post(e -> {
                try {
                    s.listener.onRequestedDeadlineMissed(new Statuses.RequestedDeadlineMissedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rIncompatible(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.RequestedIncompatibleQos status = Statuses.RequestedIncompatibleQos.of(s.env, Library.MEMORY.asSlice(p, Statuses.INCOMPATIBLE_SIZE));
            s.post(e -> {
                try {
                    s.listener.onRequestedIncompatibleQos(new Statuses.RequestedIncompatibleQosEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rRejected(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.SampleRejected status = Statuses.SampleRejected.of(s.env, Library.MEMORY.asSlice(p, Statuses.REJECTED_SIZE));
            s.post(e -> {
                try {
                    s.listener.onSampleRejected(new Statuses.SampleRejectedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rLivelinessChanged(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.LivelinessChanged status = Statuses.LivelinessChanged.of(s.env, Library.MEMORY.asSlice(p, Statuses.LIVELINESS_SIZE));
            s.post(e -> {
                try {
                    s.listener.onLivelinessChanged(new Statuses.LivelinessChangedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void rDataAvailable(long self) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.DataAvailable status = new Statuses.DataAvailable(s.env);
            s.post(e -> {
                try {
                    s.listener.onDataAvailable(new Statuses.DataAvailableEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rMatched(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.SubscriptionMatched status = Statuses.SubscriptionMatched.of(s.env, Library.MEMORY.asSlice(p, Statuses.MATCHED_SIZE));
            s.post(e -> {
                try {
                    s.listener.onSubscriptionMatched(new Statuses.SubscriptionMatchedEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void rSampleLost(long self, long p) {
        try {
            Slot<ReaderImpl<Object>, DataReaderListener<Object>> s = slot(self);
            if (s == null) {
                return;
            }
            Statuses.SampleLost status = Statuses.SampleLost.of(s.env, Library.MEMORY.asSlice(p, Statuses.COUNT_SIZE));
            s.post(e -> {
                try {
                    s.listener.onSampleLost(new Statuses.SampleLostEv<Object>(e, status));
                } catch (Throwable t) {
                    report(t);
                }
            });
        } catch (Throwable t) {
            report(t);
        }
    }

    private static void release(long self) {
        unregister(self);
    }

    /** An rdds_writer_listener calling the slot numbered id. */
    static MemorySegment writerTable(Arena arena, long id) {
        MemorySegment t = arena.allocate(6 * 8L, 8);
        t.set(JAVA_LONG, 0, id);
        t.set(JAVA_LONG, 8, WDEADLINE);
        t.set(JAVA_LONG, 16, WINCOMPATIBLE);
        t.set(JAVA_LONG, 24, WLIVELINESSLOST);
        t.set(JAVA_LONG, 32, WMATCHED);
        t.set(JAVA_LONG, 40, RELEASE);
        return t;
    }

    /** An rdds_reader_listener calling the slot numbered id. */
    static MemorySegment readerTable(Arena arena, long id) {
        MemorySegment t = arena.allocate(9 * 8L, 8);
        t.set(JAVA_LONG, 0, id);
        t.set(JAVA_LONG, 8, RDEADLINE);
        t.set(JAVA_LONG, 16, RINCOMPATIBLE);
        t.set(JAVA_LONG, 24, RREJECTED);
        t.set(JAVA_LONG, 32, RLIVELINESSCHANGED);
        t.set(JAVA_LONG, 40, RDATAAVAILABLE);
        t.set(JAVA_LONG, 48, RMATCHED);
        t.set(JAVA_LONG, 56, RSAMPLELOST);
        t.set(JAVA_LONG, 64, RELEASE);
        return t;
    }
}
