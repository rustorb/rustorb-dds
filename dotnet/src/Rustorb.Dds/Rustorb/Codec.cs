// What a topic type is to rustorb-dds: its DDS-XTypes type and how its
// values are written to and read from the streams. rustorb-idl -bcsharp
// -Wbdds writes a <Name>Codec per struct, union and enum and registers it
// when the assembly is loaded.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using System.Collections.Concurrent;
using System.Runtime.CompilerServices;

namespace Rustorb.Dds;

/// <summary>The type support of T.</summary>
public interface ICodec<T>
{
    /// <summary>The DDS-XTypes type.</summary>
    TypeRef Type { get; }

    /// <summary>The name the type is registered under (the IDL's scoped
    /// name, "M::T").</summary>
    string TypeName { get; }

    /// <summary>A new value: strings empty, sequences without elements,
    /// arrays of their length, unions with their first member.</summary>
    T NewValue();

    void Write(Out output, T value);

    T Read(In input);
}

/// <summary>The codecs of the topic types.</summary>
public static class TypeSupport
{
    private static readonly ConcurrentDictionary<Type, object> codecs = new();

    /// <summary>Registers the codec of T (the generated code calls it).</summary>
    public static void Register<T>(ICodec<T> codec)
    {
        codecs[typeof(T)] = codec ?? throw new ArgumentNullException(nameof(codec));
    }

    /// <summary>The codec of T; ArgumentException when there is none.</summary>
    public static ICodec<T> Get<T>()
    {
        if (codecs.TryGetValue(typeof(T), out object? c))
        {
            return (ICodec<T>)c;
        }
        // The generated code registers its codecs in a module initializer,
        // which runs when its module is first used.
        RuntimeHelpers.RunModuleConstructor(typeof(T).Module.ModuleHandle);
        if (codecs.TryGetValue(typeof(T), out c))
        {
            return (ICodec<T>)c;
        }
        throw new ArgumentException(
            $"{typeof(T)} has no DDS type support: generate it with rustorb-idl -bcsharp -Wbdds or -Wbdds_only");
    }

    /// <summary>The serialized payload of a sample, with its encapsulation
    /// header (XCDR1, or XCDR2 when xcdr2).</summary>
    public static unsafe byte[] Serialize<T>(T value, bool xcdr2 = false)
    {
        ICodec<T> codec = Get<T>();
        using var o = new Out();
        codec.Write(o, value);
        TypeRef t = codec.Type;
        byte* data;
        nuint len;
        Native.Errors.Check(Native.Ffi.rdds_type_serialize(t.Raw, o.Raw, (byte)(xcdr2 ? 1 : 0), &data, &len));
        GC.KeepAlive(t);
        GC.KeepAlive(o);
        try
        {
            return new ReadOnlySpan<byte>(data, checked((int)len)).ToArray();
        }
        finally
        {
            Native.Ffi.rdds_bytes_free(data, len);
        }
    }

    /// <summary>A sample from its serialized payload.</summary>
    public static unsafe T Deserialize<T>(byte[] data)
    {
        ICodec<T> codec = Get<T>();
        TypeRef t = codec.Type;
        nint h;
        fixed (byte* p = data)
        {
            Native.Errors.Check(Native.Ffi.rdds_type_deserialize(t.Raw, p, (nuint)data.Length, &h));
        }
        GC.KeepAlive(t);
        using var i = new In(h);
        T v = codec.Read(i);
        i.Check();
        return v;
    }
}
