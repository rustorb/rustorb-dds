// The input stream of a sample (rdds_in): the values in the order of the
// type. A read that does not match the type returns a zero value and makes
// Check throw.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using Rustorb.Dds.Native;

namespace Rustorb.Dds;

/// <summary>The values of a sample, to read. Not thread-safe.</summary>
public sealed unsafe class In : IDisposable
{
    internal readonly InHandle handle;

    internal In(nint h)
    {
        handle = new InHandle(h);
    }

    internal nint Raw => handle.Raw;

    public void Dispose() => handle.Dispose();

    /// <summary>Throws ArgumentException if a read did not match the type.</summary>
    public void Check() => Errors.Check(Ffi.rdds_in_status(Raw));

    /// <summary>Whether every value was read.</summary>
    public bool AtEnd => Ffi.rdds_in_at_end(Raw) != 0;

    public bool ReadBoolean() => Ffi.rdds_in_boolean(Raw) != 0;
    public byte ReadOctet() => Ffi.rdds_in_octet(Raw);
    public sbyte ReadInt8() => Ffi.rdds_in_int8(Raw);
    public byte ReadUint8() => Ffi.rdds_in_uint8(Raw);
    public short ReadInt16() => Ffi.rdds_in_int16(Raw);
    public ushort ReadUint16() => Ffi.rdds_in_uint16(Raw);
    public int ReadInt32() => Ffi.rdds_in_int32(Raw);
    public uint ReadUint32() => Ffi.rdds_in_uint32(Raw);
    public long ReadInt64() => Ffi.rdds_in_int64(Raw);
    public ulong ReadUint64() => Ffi.rdds_in_uint64(Raw);
    public float ReadFloat32() => Ffi.rdds_in_float32(Raw);
    public double ReadFloat64() => Ffi.rdds_in_float64(Raw);

    /// <summary>A long double as a decimal, rounded to nearest even;
    /// OverflowException out of decimal's range.</summary>
    public decimal ReadFloat128()
    {
        ulong hi, lo;
        Ffi.rdds_in_float128(Raw, &hi, &lo);
        return Float128.ToDecimal(hi, lo);
    }

    public char ReadChar8() => (char)Ffi.rdds_in_char8(Raw);
    public char ReadChar16() => (char)Ffi.rdds_in_char16(Raw);

    public string ReadString()
    {
        byte* p;
        nuint n;
        Ffi.rdds_in_string(Raw, &p, &n);
        return Utf8.Read(p, n);
    }

    public string ReadWstring()
    {
        ushort* p;
        nuint n;
        Ffi.rdds_in_wstring(Raw, &p, &n);
        return n == 0 ? "" : new string((char*)p, 0, checked((int)n));
    }

    public int ReadLength() => checked((int)Ffi.rdds_in_length(Raw));
    public bool ReadPresent() => Ffi.rdds_in_present(Raw) != 0;
    public long ReadDiscriminator() => Ffi.rdds_in_discriminator(Raw);
    public int ReadEnum() => Ffi.rdds_in_enum(Raw);
    public ulong ReadBitmask() => Ffi.rdds_in_bitmask(Raw);

    // Exactly n elements: the length read before, or the array's size.

    public bool[] ReadBooleanArray(int n)
    {
        var v = new bool[n];
        fixed (bool* p = v)
        {
            Ffi.rdds_in_boolean_array(Raw, (byte*)p, (nuint)n);
        }
        return v;
    }

    public byte[] ReadOctetArray(int n)
    {
        var v = new byte[n];
        fixed (byte* p = v)
        {
            Ffi.rdds_in_octet_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public sbyte[] ReadInt8Array(int n)
    {
        var v = new sbyte[n];
        fixed (sbyte* p = v)
        {
            Ffi.rdds_in_int8_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public byte[] ReadUint8Array(int n)
    {
        var v = new byte[n];
        fixed (byte* p = v)
        {
            Ffi.rdds_in_uint8_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public short[] ReadInt16Array(int n)
    {
        var v = new short[n];
        fixed (short* p = v)
        {
            Ffi.rdds_in_int16_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public ushort[] ReadUint16Array(int n)
    {
        var v = new ushort[n];
        fixed (ushort* p = v)
        {
            Ffi.rdds_in_uint16_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public int[] ReadInt32Array(int n)
    {
        var v = new int[n];
        fixed (int* p = v)
        {
            Ffi.rdds_in_int32_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public uint[] ReadUint32Array(int n)
    {
        var v = new uint[n];
        fixed (uint* p = v)
        {
            Ffi.rdds_in_uint32_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public long[] ReadInt64Array(int n)
    {
        var v = new long[n];
        fixed (long* p = v)
        {
            Ffi.rdds_in_int64_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public ulong[] ReadUint64Array(int n)
    {
        var v = new ulong[n];
        fixed (ulong* p = v)
        {
            Ffi.rdds_in_uint64_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public float[] ReadFloat32Array(int n)
    {
        var v = new float[n];
        fixed (float* p = v)
        {
            Ffi.rdds_in_float32_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public double[] ReadFloat64Array(int n)
    {
        var v = new double[n];
        fixed (double* p = v)
        {
            Ffi.rdds_in_float64_array(Raw, p, (nuint)n);
        }
        return v;
    }

    public char[] ReadChar8Array(int n)
    {
        var b = new byte[n];
        fixed (byte* p = b)
        {
            Ffi.rdds_in_char8_array(Raw, p, (nuint)n);
        }
        var v = new char[n];
        for (int i = 0; i < n; i++)
        {
            v[i] = (char)b[i];
        }
        return v;
    }

    public char[] ReadChar16Array(int n)
    {
        var v = new char[n];
        fixed (char* p = v)
        {
            Ffi.rdds_in_char16_array(Raw, (ushort*)p, (nuint)n);
        }
        return v;
    }

    /// <summary>A jagged array of the dimensions from its elements in row
    /// order (Out.Flatten's inverse).</summary>
    public static Array Unflatten<T>(T[] flat, params int[] dims)
    {
        int at = 0;
        return Build(flat, dims, 0, ref at);
    }

    private static Array Build<T>(T[] flat, int[] dims, int level, ref int at)
    {
        if (level == dims.Length - 1)
        {
            var leaf = new T[dims[level]];
            Array.Copy(flat, at, leaf, 0, leaf.Length);
            at += leaf.Length;
            return leaf;
        }
        Type element = typeof(T);
        for (int i = level + 1; i < dims.Length; i++)
        {
            element = element.MakeArrayType();
        }
        Array a = Array.CreateInstance(element, dims[level]);
        for (int i = 0; i < dims[level]; i++)
        {
            a.SetValue(Build(flat, dims, level + 1, ref at), i);
        }
        return a;
    }
}
