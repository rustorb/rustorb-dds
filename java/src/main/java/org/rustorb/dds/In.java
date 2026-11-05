// The input stream of a sample's values (rdds_in).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

/**
 * The values of a sample, read in the order of its type (an
 * {@code rdds_in}). A read that does not fit the sample returns zero and
 * makes {@link #check} throw. Not thread-safe; close it to free it.
 */
public final class In implements AutoCloseable {
    private long handle;
    private final Arena arena = Arena.ofConfined();
    private final MemorySegment scratch = arena.allocate(16);

    In(long handle) {
        this.handle = handle;
    }

    private long handle() {
        if (handle == 0) {
            throw new IllegalStateException("the stream is closed");
        }
        return handle;
    }

    @Override
    public void close() {
        long h = handle;
        handle = 0;
        if (h != 0) {
            arena.close();
            try {
                Ffi.rdds_in_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    /** Throws IllegalArgumentException if a read did not fit the sample
     * or values are left. */
    public void check() {
        try {
            int rc = (int) Ffi.rdds_in_status.invokeExact(handle());
            Errors.check(null, rc);
            if (!(boolean) Ffi.rdds_in_at_end.invokeExact(handle())) {
                throw new IllegalArgumentException("the sample has more values than the type read");
            }
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public boolean readBoolean() {
        try {
            return (boolean) Ffi.rdds_in_boolean.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public byte readOctet() {
        try {
            return (byte) Ffi.rdds_in_octet.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public byte readInt8() {
        try {
            return (byte) Ffi.rdds_in_int8.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public byte readUint8() {
        try {
            return (byte) Ffi.rdds_in_uint8.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public short readInt16() {
        try {
            return (short) Ffi.rdds_in_int16.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public short readUint16() {
        try {
            return (short) Ffi.rdds_in_uint16.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public int readInt32() {
        try {
            return (int) Ffi.rdds_in_int32.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public int readUint32() {
        try {
            return (int) Ffi.rdds_in_uint32.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public long readInt64() {
        try {
            return (long) Ffi.rdds_in_int64.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public long readUint64() {
        try {
            return (long) Ffi.rdds_in_uint64.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public float readFloat32() {
        try {
            return (float) Ffi.rdds_in_float32.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public double readFloat64() {
        try {
            return (double) Ffi.rdds_in_float64.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A long double, rounded to the nearest double. */
    public double readFloat128() {
        try {
            Ffi.rdds_in_float128.invokeExact(handle(), scratch.address(), scratch.address() + 8);
            return Float128.toDouble(scratch.get(JAVA_LONG, 0), scratch.get(JAVA_LONG, 8));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public char readChar8() {
        try {
            return (char) ((byte) Ffi.rdds_in_char8.invokeExact(handle()) & 0xff);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public char readChar16() {
        try {
            return (char) (short) Ffi.rdds_in_char16.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public String readString() {
        try {
            Ffi.rdds_in_string.invokeExact(handle(), scratch.address(), scratch.address() + 8);
            return Library.utf8(scratch.get(JAVA_LONG, 0), scratch.get(JAVA_LONG, 8));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public String readWstring() {
        try {
            Ffi.rdds_in_wstring.invokeExact(handle(), scratch.address(), scratch.address() + 8);
            long p = scratch.get(JAVA_LONG, 0);
            int n = Math.toIntExact(scratch.get(JAVA_LONG, 8));
            char[] c = new char[n];
            if (n > 0) {
                MemorySegment.copy(Library.MEMORY, java.lang.foreign.ValueLayout.JAVA_CHAR_UNALIGNED, p, c, 0, n);
            }
            return new String(c);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The length of a sequence or map, before its elements;
     * IllegalArgumentException for one Java arrays cannot have. */
    public int readLength() {
        int n;
        try {
            n = (int) Ffi.rdds_in_length.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        if (n < 0) {
            throw new IllegalArgumentException("a sequence of " + Integer.toUnsignedString(n) + " elements");
        }
        return n;
    }

    public boolean readPresent() {
        try {
            return (boolean) Ffi.rdds_in_present.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public long readDiscriminator() {
        try {
            return (long) Ffi.rdds_in_discriminator.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public int readEnum() {
        try {
            return (int) Ffi.rdds_in_enum.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public long readBitmask() {
        try {
            return (long) Ffi.rdds_in_bitmask.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The n elements of a sequence (after its length) or array of boolean. */
    public boolean[] readBooleanArray(int n) {
        byte[] b = new byte[n];
        try {
            Ffi.rdds_in_boolean_array.invokeExact(handle(), MemorySegment.ofArray(b), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        boolean[] v = new boolean[n];
        for (int i = 0; i < n; i++) {
            v[i] = b[i] != 0;
        }
        return v;
    }

    public char[] readChar8Array(int n) {
        byte[] b = new byte[n];
        try {
            Ffi.rdds_in_char8_array.invokeExact(handle(), MemorySegment.ofArray(b), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        char[] v = new char[n];
        for (int i = 0; i < n; i++) {
            v[i] = (char) (b[i] & 0xff);
        }
        return v;
    }

    public char[] readChar16Array(int n) {
        char[] v = new char[n];
        try {
            Ffi.rdds_in_char16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public byte[] readOctetArray(int n) {
        byte[] v = new byte[n];
        try {
            Ffi.rdds_in_octet_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public byte[] readInt8Array(int n) {
        byte[] v = new byte[n];
        try {
            Ffi.rdds_in_int8_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public byte[] readUint8Array(int n) {
        byte[] v = new byte[n];
        try {
            Ffi.rdds_in_uint8_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public short[] readInt16Array(int n) {
        short[] v = new short[n];
        try {
            Ffi.rdds_in_int16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public short[] readUint16Array(int n) {
        short[] v = new short[n];
        try {
            Ffi.rdds_in_uint16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public int[] readInt32Array(int n) {
        int[] v = new int[n];
        try {
            Ffi.rdds_in_int32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public int[] readUint32Array(int n) {
        int[] v = new int[n];
        try {
            Ffi.rdds_in_uint32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public long[] readInt64Array(int n) {
        long[] v = new long[n];
        try {
            Ffi.rdds_in_int64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public long[] readUint64Array(int n) {
        long[] v = new long[n];
        try {
            Ffi.rdds_in_uint64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public float[] readFloat32Array(int n) {
        float[] v = new float[n];
        try {
            Ffi.rdds_in_float32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    public double[] readFloat64Array(int n) {
        double[] v = new double[n];
        try {
            Ffi.rdds_in_float64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) n);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
        return v;
    }

    /** The array of the dimensions given, from its elements row by row in
     * flat (flat itself when there is one dimension). */
    public static Object unflatten(Object flat, int... dims) {
        if (dims.length == 1) {
            return flat;
        }
        return copyIn(flat, flat.getClass().getComponentType(), dims, 0, new int[] {0});
    }

    private static Object copyIn(Object flat, Class<?> component, int[] dims, int level, int[] at) {
        int[] rest = java.util.Arrays.copyOfRange(dims, level, dims.length);
        Object a = java.lang.reflect.Array.newInstance(component, rest);
        if (level == dims.length - 1) {
            System.arraycopy(flat, at[0], a, 0, dims[level]);
            at[0] += dims[level];
            return a;
        }
        for (int i = 0; i < dims[level]; i++) {
            java.lang.reflect.Array.set(a, i, copyIn(flat, component, dims, level + 1, at));
        }
        return a;
    }
}
