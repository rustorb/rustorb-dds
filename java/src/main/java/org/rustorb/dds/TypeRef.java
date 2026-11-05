// A DDS-XTypes type built through the C ABI.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_BYTE;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.ref.Cleaner;

/** A DDS-XTypes type (an {@code rdds_type}), made by {@link Types}. */
public final class TypeRef {
    private static final Cleaner CLEANER = Cleaner.create();

    private final long handle;

    TypeRef(long handle) {
        this.handle = handle;
        CLEANER.register(this, () -> release(handle));
    }

    private static void release(long handle) {
        try {
            Ffi.rdds_type_release.invokeExact(handle);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    long handle() {
        return handle;
    }

    /** The 14-octet equivalence hash of the minimal TypeObject
     * (DDS-XTypes 1.3, 7.3.4.9). */
    public byte[] minimalHash() {
        return hashes()[0];
    }

    /** The 14-octet equivalence hash of the complete TypeObject. */
    public byte[] completeHash() {
        return hashes()[1];
    }

    private byte[][] hashes() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment m = arena.allocate(14);
            MemorySegment c = arena.allocate(14);
            int rc = (int) Ffi.rdds_type_hashes.invokeExact(handle, m.address(), c.address());
            Errors.check(null, rc);
            return new byte[][] {m.toArray(JAVA_BYTE), c.toArray(JAVA_BYTE)};
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The XCDR (or XCDR2) encoding of the sample in out, with its
     * encapsulation header, little endian; for tests and tools. */
    public byte[] serialize(Out out, boolean xcdr2) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment p = arena.allocate(16);
            int rc = (int) Ffi.rdds_type_serialize.invokeExact(handle, out.handle(), xcdr2, p.address(),
                    p.address() + 8);
            Errors.check(null, rc);
            long data = p.get(java.lang.foreign.ValueLayout.JAVA_LONG, 0);
            long len = p.get(java.lang.foreign.ValueLayout.JAVA_LONG, 8);
            byte[] b = new byte[Math.toIntExact(len)];
            MemorySegment.copy(Library.MEMORY, JAVA_BYTE, data, b, 0, b.length);
            Ffi.rdds_bytes_free.invokeExact(data, len);
            return b;
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The values of a sample serialized as {@link #serialize} does. */
    public In deserialize(byte[] data) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment d = arena.allocateFrom(JAVA_BYTE, data);
            MemorySegment p = arena.allocate(8);
            int rc = (int) Ffi.rdds_type_deserialize.invokeExact(handle, d.address(), (long) data.length,
                    p.address());
            Errors.check(null, rc);
            return new In(p.get(java.lang.foreign.ValueLayout.JAVA_LONG, 0));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }
}
