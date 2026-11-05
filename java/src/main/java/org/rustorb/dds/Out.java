// The output stream of a sample's values (rdds_out).
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.nio.charset.StandardCharsets;

/**
 * The values of a sample, written in the order of its type (an
 * {@code rdds_out}; see docs/c-abi.md, "Samples"). The stream does not
 * know the type: a writer checks the values against the type of its
 * topic and throws IllegalArgumentException when they do not fit. Unsigned
 * IDL integers are the Java integers of the same size, taken as unsigned.
 * Not thread-safe; close it to free it.
 */
public final class Out implements AutoCloseable {
    private long handle;

    public Out() {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment p = arena.allocate(JAVA_LONG);
            int rc = (int) Ffi.rdds_out_new.invokeExact(p.address());
            Errors.check(null, rc);
            handle = p.get(JAVA_LONG, 0);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    long handle() {
        if (handle == 0) {
            throw new IllegalStateException("the stream is closed");
        }
        return handle;
    }

    /** Empties the stream, to write the next sample. */
    public void clear() {
        try {
            Ffi.rdds_out_clear.invokeExact(handle());
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    @Override
    public void close() {
        long h = handle;
        handle = 0;
        if (h != 0) {
            try {
                Ffi.rdds_out_release.invokeExact(h);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    public void writeBoolean(boolean v) {
        try {
            Ffi.rdds_out_boolean.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeOctet(byte v) {
        try {
            Ffi.rdds_out_octet.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt8(byte v) {
        try {
            Ffi.rdds_out_int8.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint8(byte v) {
        try {
            Ffi.rdds_out_uint8.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt16(short v) {
        try {
            Ffi.rdds_out_int16.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint16(short v) {
        try {
            Ffi.rdds_out_uint16.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt32(int v) {
        try {
            Ffi.rdds_out_int32.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint32(int v) {
        try {
            Ffi.rdds_out_uint32.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt64(long v) {
        try {
            Ffi.rdds_out_int64.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint64(long v) {
        try {
            Ffi.rdds_out_uint64.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeFloat32(float v) {
        try {
            Ffi.rdds_out_float32.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeFloat64(double v) {
        try {
            Ffi.rdds_out_float64.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A long double: the binary128 equal to v. */
    public void writeFloat128(double v) {
        try {
            Ffi.rdds_out_float128.invokeExact(handle(), Float128.high(v), Float128.low(v));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A char (ISO-8859-1): IllegalArgumentException above U+00FF. */
    public void writeChar8(char v) {
        try {
            Ffi.rdds_out_char8.invokeExact(handle(), latin1(v));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A wchar: one UTF-16 code unit. */
    public void writeChar16(char v) {
        try {
            Ffi.rdds_out_char16.invokeExact(handle(), (short) v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A string (UTF-8 on the wire); null is IllegalArgumentException. */
    public void writeString(String v) {
        byte[] b = notNull(v, "string").getBytes(StandardCharsets.UTF_8);
        try {
            Ffi.rdds_out_string.invokeExact(handle(), MemorySegment.ofArray(b), (long) b.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A wstring (UTF-16 code units). */
    public void writeWstring(String v) {
        char[] c = notNull(v, "wstring").toCharArray();
        try {
            Ffi.rdds_out_wstring.invokeExact(handle(), MemorySegment.ofArray(c), (long) c.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The length of a sequence or map, before its elements. */
    public void writeLength(int v) {
        try {
            Ffi.rdds_out_length.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** Whether an optional member is present, before its value. */
    public void writePresent(boolean v) {
        try {
            Ffi.rdds_out_present.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The discriminator of a union (its integer value). */
    public void writeDiscriminator(long v) {
        try {
            Ffi.rdds_out_discriminator.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** An enum: the value of its literal. */
    public void writeEnum(int v) {
        try {
            Ffi.rdds_out_enum.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A bitmask: its bits. */
    public void writeBitmask(long v) {
        try {
            Ffi.rdds_out_bitmask.invokeExact(handle(), v);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The elements of a sequence or array of boolean, in one call. */
    public void writeBooleanArray(boolean[] v) {
        byte[] b = new byte[v.length];
        for (int i = 0; i < v.length; i++) {
            b[i] = v[i] ? (byte) 1 : (byte) 0;
        }
        try {
            Ffi.rdds_out_boolean_array.invokeExact(handle(), MemorySegment.ofArray(b), (long) b.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The elements of a sequence or array of char (ISO-8859-1). */
    public void writeChar8Array(char[] v) {
        byte[] b = new byte[v.length];
        for (int i = 0; i < v.length; i++) {
            b[i] = latin1(v[i]);
        }
        try {
            Ffi.rdds_out_char8_array.invokeExact(handle(), MemorySegment.ofArray(b), (long) b.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** The elements of a sequence or array of wchar. */
    public void writeChar16Array(char[] v) {
        try {
            Ffi.rdds_out_char16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeOctetArray(byte[] v) {
        try {
            Ffi.rdds_out_octet_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt8Array(byte[] v) {
        try {
            Ffi.rdds_out_int8_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint8Array(byte[] v) {
        try {
            Ffi.rdds_out_uint8_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt16Array(short[] v) {
        try {
            Ffi.rdds_out_int16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint16Array(short[] v) {
        try {
            Ffi.rdds_out_uint16_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt32Array(int[] v) {
        try {
            Ffi.rdds_out_int32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint32Array(int[] v) {
        try {
            Ffi.rdds_out_uint32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeInt64Array(long[] v) {
        try {
            Ffi.rdds_out_int64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeUint64Array(long[] v) {
        try {
            Ffi.rdds_out_uint64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeFloat32Array(float[] v) {
        try {
            Ffi.rdds_out_float32_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public void writeFloat64Array(double[] v) {
        try {
            Ffi.rdds_out_float64_array.invokeExact(handle(), MemorySegment.ofArray(v), (long) v.length);
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /**
     * The elements of an array of a primitive type of the dimensions
     * given, row by row, in one Java array of that type (the array itself
     * when it has one dimension). IllegalArgumentException if a dimension
     * has another length.
     */
    public static Object flatten(Object array, int... dims) {
        int total = 1;
        for (int d : dims) {
            total *= d;
        }
        Class<?> component = array == null ? null : array.getClass();
        for (int i = 0; component != null && i < dims.length; i++) {
            component = component.getComponentType();
        }
        if (component == null || !component.isPrimitive()) {
            throw new IllegalArgumentException("an array of " + dims.length + " dimensions was expected");
        }
        if (dims.length == 1) {
            checkLength(array, dims[0]);
            return array;
        }
        Object flat = java.lang.reflect.Array.newInstance(component, total);
        copyOut(array, dims, 0, flat, 0);
        return flat;
    }

    private static int copyOut(Object array, int[] dims, int level, Object flat, int at) {
        checkLength(array, dims[level]);
        if (level == dims.length - 1) {
            System.arraycopy(array, 0, flat, at, dims[level]);
            return at + dims[level];
        }
        for (int i = 0; i < dims[level]; i++) {
            at = copyOut(java.lang.reflect.Array.get(array, i), dims, level + 1, flat, at);
        }
        return at;
    }

    /** IllegalArgumentException unless array is an array of length
     * elements. */
    public static void checkLength(Object array, int length) {
        if (array == null) {
            throw new IllegalArgumentException("a null array in a sample");
        }
        int n = java.lang.reflect.Array.getLength(array);
        if (n != length) {
            throw new IllegalArgumentException("an array of " + n + " elements where the type has " + length);
        }
    }

    private static byte latin1(char c) {
        if (c > 0xff) {
            throw new IllegalArgumentException(
                    "the char U+" + Integer.toHexString(c) + " is not in ISO-8859-1");
        }
        return (byte) c;
    }

    private static <T> T notNull(T v, String what) {
        if (v == null) {
            throw new IllegalArgumentException("a null " + what + " in a sample");
        }
        return v;
    }
}
