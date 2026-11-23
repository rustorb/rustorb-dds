// The output stream of a sample (rdds_out): the values in the order of the
// type (docs/c-abi.md, "サンプル：値のストリーム"). The generated code
// writes them; the library encodes them when the writer writes.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using Rustorb.Dds.Native;

namespace Rustorb.Dds;

/// <summary>The values of a sample, to write. Not thread-safe.</summary>
public sealed unsafe class Out : IDisposable
{
    internal readonly OutHandle handle;

    public Out()
    {
        Library.Check();
        nint h;
        Errors.Check(Ffi.rdds_out_new(&h));
        handle = new OutHandle(h);
    }

    internal nint Raw => handle.Raw;

    /// <summary>Empties the stream, to write the next sample.</summary>
    public void Clear() => Ffi.rdds_out_clear(Raw);

    public void Dispose() => handle.Dispose();

    public void WriteBoolean(bool v) => Ffi.rdds_out_boolean(Raw, (byte)(v ? 1 : 0));
    public void WriteOctet(byte v) => Ffi.rdds_out_octet(Raw, v);
    public void WriteInt8(sbyte v) => Ffi.rdds_out_int8(Raw, v);
    public void WriteUint8(byte v) => Ffi.rdds_out_uint8(Raw, v);
    public void WriteInt16(short v) => Ffi.rdds_out_int16(Raw, v);
    public void WriteUint16(ushort v) => Ffi.rdds_out_uint16(Raw, v);
    public void WriteInt32(int v) => Ffi.rdds_out_int32(Raw, v);
    public void WriteUint32(uint v) => Ffi.rdds_out_uint32(Raw, v);
    public void WriteInt64(long v) => Ffi.rdds_out_int64(Raw, v);
    public void WriteUint64(ulong v) => Ffi.rdds_out_uint64(Raw, v);
    public void WriteFloat32(float v) => Ffi.rdds_out_float32(Raw, v);
    public void WriteFloat64(double v) => Ffi.rdds_out_float64(Raw, v);

    /// <summary>A long double, as binary128 rounded to nearest even.</summary>
    public void WriteFloat128(decimal v)
    {
        var (hi, lo) = Float128.FromDecimal(v);
        Ffi.rdds_out_float128(Raw, hi, lo);
    }

    /// <summary>A char (ISO-8859-1): ArgumentException above U+00FF.</summary>
    public void WriteChar8(char v) => Ffi.rdds_out_char8(Raw, Latin1(v));

    /// <summary>A wchar: one UTF-16 code unit.</summary>
    public void WriteChar16(char v) => Ffi.rdds_out_char16(Raw, v);

    public void WriteString(string v)
    {
        NotNull(v);
        byte[] b = Utf8.Bytes(v);
        fixed (byte* p = b)
        {
            Ffi.rdds_out_string(Raw, p, (nuint)b.Length);
        }
    }

    public void WriteWstring(string v)
    {
        NotNull(v);
        fixed (char* p = v)
        {
            Ffi.rdds_out_wstring(Raw, (ushort*)p, (nuint)v.Length);
        }
    }

    /// <summary>The length of a sequence or map, before its elements.</summary>
    public void WriteLength(int v) => Ffi.rdds_out_length(Raw, checked((uint)v));

    /// <summary>Whether an @optional member is there, before its value.</summary>
    public void WritePresent(bool v) => Ffi.rdds_out_present(Raw, (byte)(v ? 1 : 0));

    /// <summary>A union's discriminator, as an integer (an enum's value, 1 for TRUE).</summary>
    public void WriteDiscriminator(long v) => Ffi.rdds_out_discriminator(Raw, v);

    /// <summary>The value of an enumerator.</summary>
    public void WriteEnum(int v) => Ffi.rdds_out_enum(Raw, v);

    /// <summary>The bits of a bitmask or bitset.</summary>
    public void WriteBitmask(ulong v) => Ffi.rdds_out_bitmask(Raw, v);

    // The elements of sequences and arrays of basic types, at once.

    public void WriteBooleanArray(IList<bool> v)
    {
        ReadOnlySpan<bool> s = Span(v);
        fixed (bool* p = s)
        {
            Ffi.rdds_out_boolean_array(Raw, (byte*)p, (nuint)s.Length);
        }
    }

    public void WriteOctetArray(IList<byte> v)
    {
        ReadOnlySpan<byte> s = Span(v);
        fixed (byte* p = s)
        {
            Ffi.rdds_out_octet_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteInt8Array(IList<sbyte> v)
    {
        ReadOnlySpan<sbyte> s = Span(v);
        fixed (sbyte* p = s)
        {
            Ffi.rdds_out_int8_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteUint8Array(IList<byte> v)
    {
        ReadOnlySpan<byte> s = Span(v);
        fixed (byte* p = s)
        {
            Ffi.rdds_out_uint8_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteInt16Array(IList<short> v)
    {
        ReadOnlySpan<short> s = Span(v);
        fixed (short* p = s)
        {
            Ffi.rdds_out_int16_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteUint16Array(IList<ushort> v)
    {
        ReadOnlySpan<ushort> s = Span(v);
        fixed (ushort* p = s)
        {
            Ffi.rdds_out_uint16_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteInt32Array(IList<int> v)
    {
        ReadOnlySpan<int> s = Span(v);
        fixed (int* p = s)
        {
            Ffi.rdds_out_int32_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteUint32Array(IList<uint> v)
    {
        ReadOnlySpan<uint> s = Span(v);
        fixed (uint* p = s)
        {
            Ffi.rdds_out_uint32_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteInt64Array(IList<long> v)
    {
        ReadOnlySpan<long> s = Span(v);
        fixed (long* p = s)
        {
            Ffi.rdds_out_int64_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteUint64Array(IList<ulong> v)
    {
        ReadOnlySpan<ulong> s = Span(v);
        fixed (ulong* p = s)
        {
            Ffi.rdds_out_uint64_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteFloat32Array(IList<float> v)
    {
        ReadOnlySpan<float> s = Span(v);
        fixed (float* p = s)
        {
            Ffi.rdds_out_float32_array(Raw, p, (nuint)s.Length);
        }
    }

    public void WriteFloat64Array(IList<double> v)
    {
        ReadOnlySpan<double> s = Span(v);
        fixed (double* p = s)
        {
            Ffi.rdds_out_float64_array(Raw, p, (nuint)s.Length);
        }
    }

    /// <summary>chars as ISO-8859-1: ArgumentException above U+00FF.</summary>
    public void WriteChar8Array(IList<char> v)
    {
        NotNull(v);
        byte[] b = new byte[v.Count];
        for (int i = 0; i < b.Length; i++)
        {
            b[i] = Latin1(v[i]);
        }
        fixed (byte* p = b)
        {
            Ffi.rdds_out_char8_array(Raw, p, (nuint)b.Length);
        }
    }

    public void WriteChar16Array(IList<char> v)
    {
        ReadOnlySpan<char> s = Span(v);
        fixed (char* p = s)
        {
            Ffi.rdds_out_char16_array(Raw, (ushort*)p, (nuint)s.Length);
        }
    }

    // Helpers of the generated code.

    /// <summary>ArgumentException unless the array has n elements.</summary>
    public static void CheckLength<T>(T[] v, int n)
    {
        NotNull(v);
        if (v.Length != n)
        {
            throw new ArgumentException($"an array of {v.Length} elements, not {n}");
        }
    }

    /// <summary>ArgumentNullException for null (strings, sequences, arrays,
    /// structs and unions are never null in a sample).</summary>
    public static void NotNull(object? v)
    {
        if (v is null)
        {
            throw new ArgumentNullException(null, "a null value in a sample");
        }
    }

    /// <summary>The elements of a multi-dimensional (jagged) array in row
    /// order, each dimension checked.</summary>
    public static T[] Flatten<T>(Array v, params int[] dims)
    {
        int total = 1;
        foreach (int d in dims)
        {
            total *= d;
        }
        var flat = new T[total];
        int at = 0;
        Walk(v, dims, 0, flat, ref at);
        return flat;
    }

    private static void Walk<T>(Array v, int[] dims, int level, T[] flat, ref int at)
    {
        NotNull(v);
        if (v.Length != dims[level])
        {
            throw new ArgumentException($"an array of {v.Length} elements, not {dims[level]}");
        }
        if (level == dims.Length - 1)
        {
            ((T[])v).CopyTo(flat, at);
            at += v.Length;
            return;
        }
        foreach (object? e in v)
        {
            Walk((Array)e!, dims, level + 1, flat, ref at);
        }
    }

    private static ReadOnlySpan<T> Span<T>(IList<T> v)
    {
        NotNull(v);
        if (v is T[] a)
        {
            return a;
        }
        if (v is List<T> l)
        {
            return CollectionsMarshal.AsSpan(l);
        }
        var copy = new T[v.Count];
        v.CopyTo(copy, 0);
        return copy;
    }

    private static byte Latin1(char c)
    {
        if (c > 'ÿ')
        {
            throw new ArgumentException($"the char U+{(int)c:X4} is not in ISO-8859-1");
        }
        return (byte)c;
    }
}
