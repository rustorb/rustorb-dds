// The DDS-XTypes types of the C ABI (rdds_type and its builders, DDS-XTypes
// 1.3 7.5.2.2), as the code rustorb-idl -bcsharp -Wbdds writes builds them.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

using System;
using Rustorb.Dds.Native;

namespace Rustorb.Dds;

/// <summary>The TypeKinds of the primitive types (DDS-XTypes 1.3, 7.3.4.6).</summary>
public enum TypeKind : byte
{
    Boolean = 0x01,
    Byte = 0x02,
    Int16 = 0x03,
    Int32 = 0x04,
    Int64 = 0x05,
    Uint16 = 0x06,
    Uint32 = 0x07,
    Uint64 = 0x08,
    Float32 = 0x09,
    Float64 = 0x0A,
    Float128 = 0x0B,
    Int8 = 0x0C,
    Uint8 = 0x0D,
    Char8 = 0x10,
    Char16 = 0x11,
}

/// <summary>The extensibility of a type (@final, @appendable, @mutable).</summary>
public enum Extensibility
{
    Final = 0,
    Appendable = 1,
    Mutable = 2,
}

/// <summary>Flags of a struct or a union.</summary>
[Flags]
public enum TypeFlags : uint
{
    None = 0,
    /// <summary>@nested.</summary>
    Nested = 1,
    /// <summary>@autoid(HASH).</summary>
    AutoidHash = 2,
}

/// <summary>Flags of a member.</summary>
[Flags]
public enum MemberFlags : uint
{
    None = 0,
    Key = 1,
    Optional = 2,
    MustUnderstand = 4,
    External = 8,
    TryConstructUseDefault = 16,
    TryConstructTrim = 32,
    /// <summary>The default member of a union.</summary>
    DefaultLabel = 64,
}

/// <summary>A DDS-XTypes type.</summary>
public sealed class TypeRef
{
    internal readonly TypeHandle handle;

    internal TypeRef(nint h)
    {
        handle = new TypeHandle(h);
    }

    internal nint Raw => handle.Raw;

    /// <summary>The 14-octet equivalence hashes of its minimal and complete
    /// TypeObjects (DDS-XTypes 7.3.4.9). Not for recursive types.</summary>
    public unsafe (byte[] Minimal, byte[] Complete) Hashes()
    {
        byte[] minimal = new byte[14];
        byte[] complete = new byte[14];
        fixed (byte* m = minimal, c = complete)
        {
            Errors.Check(Ffi.rdds_type_hashes(Raw, m, c));
        }
        GC.KeepAlive(this);
        return (minimal, complete);
    }
}

/// <summary>Makes types. Every function copies the types it is given.</summary>
public static unsafe class Types
{
    /// <summary>The version of the generated code this runtime reads; the
    /// generated code checks it (rustorb-idl's csharp_dds.py).</summary>
    public const int GENERATED_CODE_VERSION = 1;

    /// <summary>Called by the generated code: fails if it is of a version
    /// this runtime does not read.</summary>
    public static void CheckGeneratedCode(int version, string idl)
    {
        if (version != GENERATED_CODE_VERSION)
        {
            throw new NotSupportedException(
                $"the code rustorb-idl wrote for {idl} is of version {version}; this Rustorb.Dds reads version {GENERATED_CODE_VERSION}");
        }
    }

    internal static TypeRef Made(int status, nint h)
    {
        Errors.Check(status);
        return new TypeRef(h);
    }

    public static TypeRef Primitive(TypeKind kind)
    {
        Library.Check();
        nint h;
        int rc = Ffi.rdds_type_primitive((byte)kind, &h);
        return Made(rc, h);
    }

    /// <summary>A string; bound 0 for unbounded.</summary>
    public static TypeRef String(uint bound = 0)
    {
        Library.Check();
        nint h;
        int rc = Ffi.rdds_type_string(0, bound, &h);
        return Made(rc, h);
    }

    /// <summary>A wide string; bound 0 for unbounded.</summary>
    public static TypeRef WString(uint bound = 0)
    {
        Library.Check();
        nint h;
        int rc = Ffi.rdds_type_string(1, bound, &h);
        return Made(rc, h);
    }

    public static TypeRef Sequence(TypeRef element, uint bound = 0)
    {
        nint h;
        int rc = Ffi.rdds_type_sequence(element.Raw, bound, &h);
        GC.KeepAlive(element);
        return Made(rc, h);
    }

    public static TypeRef Array(TypeRef element, params uint[] dimensions)
    {
        nint h;
        int rc;
        fixed (uint* d = dimensions)
        {
            rc = Ffi.rdds_type_array(element.Raw, d, (nuint)dimensions.Length, &h);
        }
        GC.KeepAlive(element);
        return Made(rc, h);
    }

    public static TypeRef Map(TypeRef key, TypeRef element, uint bound = 0)
    {
        nint h;
        int rc = Ffi.rdds_type_map(key.Raw, element.Raw, bound, &h);
        GC.KeepAlive(key);
        GC.KeepAlive(element);
        return Made(rc, h);
    }

    /// <summary>A typedef; the name is fully scoped ("A::B::T").</summary>
    public static TypeRef Alias(string name, TypeRef related)
    {
        nint h;
        int rc;
        fixed (byte* n = Utf8.Z(name))
        {
            rc = Ffi.rdds_type_alias(n, related.Raw, &h);
        }
        GC.KeepAlive(related);
        return Made(rc, h);
    }
}

/// <summary>Builds an enum.</summary>
public sealed unsafe class EnumBuilder
{
    private nint b;

    public EnumBuilder(string name, ushort bitBound, Extensibility extensibility)
    {
        Library.Check();
        nint h;
        fixed (byte* n = Utf8.Z(name))
        {
            Errors.Check(Ffi.rdds_enum_builder_new(n, bitBound, (int)extensibility, &h));
        }
        b = h;
    }

    public EnumBuilder Literal(string name, int value, bool isDefault = false)
    {
        fixed (byte* n = Utf8.Z(name))
        {
            Check(Ffi.rdds_enum_builder_add_literal(b, n, value, (byte)(isDefault ? 1 : 0)));
        }
        return this;
    }

    public TypeRef Build()
    {
        nint h;
        int rc = Ffi.rdds_enum_builder_build(b, &h);
        Ffi.rdds_enum_builder_release(b);
        b = 0;
        return Types.Made(rc, h);
    }

    private void Check(int rc)
    {
        if (rc != Status.OK)
        {
            Exception e = Errors.Of(rc);
            Ffi.rdds_enum_builder_release(b);
            b = 0;
            throw e;
        }
    }
}

/// <summary>Builds a bitmask.</summary>
public sealed unsafe class BitmaskBuilder
{
    private nint b;

    public BitmaskBuilder(string name, ushort bitBound, Extensibility extensibility)
    {
        Library.Check();
        nint h;
        fixed (byte* n = Utf8.Z(name))
        {
            Errors.Check(Ffi.rdds_bitmask_builder_new(n, bitBound, (int)extensibility, &h));
        }
        b = h;
    }

    public BitmaskBuilder Flag(string name, ushort position)
    {
        int rc;
        fixed (byte* n = Utf8.Z(name))
        {
            rc = Ffi.rdds_bitmask_builder_add_flag(b, n, position);
        }
        if (rc != Status.OK)
        {
            Exception e = Errors.Of(rc);
            Ffi.rdds_bitmask_builder_release(b);
            b = 0;
            throw e;
        }
        return this;
    }

    public TypeRef Build()
    {
        nint h;
        int rc = Ffi.rdds_bitmask_builder_build(b, &h);
        Ffi.rdds_bitmask_builder_release(b);
        b = 0;
        return Types.Made(rc, h);
    }
}

/// <summary>Builds a bitset: fields of bitcount bits from position, each
/// read and written as its holder type.</summary>
public sealed unsafe class BitsetBuilder
{
    private nint b;

    public BitsetBuilder(string name)
    {
        Library.Check();
        nint h;
        fixed (byte* n = Utf8.Z(name))
        {
            Errors.Check(Ffi.rdds_bitset_builder_new(n, &h));
        }
        b = h;
    }

    public BitsetBuilder Field(string name, ushort position, byte bitcount, TypeKind holder)
    {
        int rc;
        fixed (byte* n = Utf8.Z(name))
        {
            rc = Ffi.rdds_bitset_builder_add_field(b, n, position, bitcount, (byte)holder);
        }
        if (rc != Status.OK)
        {
            Exception e = Errors.Of(rc);
            Ffi.rdds_bitset_builder_release(b);
            b = 0;
            throw e;
        }
        return this;
    }

    public TypeRef Build()
    {
        nint h;
        int rc = Ffi.rdds_bitset_builder_build(b, &h);
        Ffi.rdds_bitset_builder_release(b);
        b = 0;
        return Types.Made(rc, h);
    }
}

/// <summary>Builds a struct.</summary>
public sealed unsafe class StructBuilder
{
    private nint b;

    public StructBuilder(string name, Extensibility extensibility, TypeFlags flags = TypeFlags.None,
        TypeRef? baseType = null)
    {
        Library.Check();
        nint h;
        int rc;
        fixed (byte* n = Utf8.Z(name))
        {
            rc = Ffi.rdds_struct_builder_new(n, (int)extensibility, (uint)flags,
                baseType is null ? 0 : baseType.Raw, &h);
        }
        GC.KeepAlive(baseType);
        Errors.Check(rc);
        b = h;
    }

    /// <summary>A member; hashId is the string of @hashid ("" when none is
    /// given), or null.</summary>
    public StructBuilder Member(string name, uint id, TypeRef type, MemberFlags flags = MemberFlags.None,
        string? hashId = null)
    {
        int rc;
        fixed (byte* n = Utf8.Z(name), hid = hashId is null ? null : Utf8.Z(hashId))
        {
            rc = Ffi.rdds_struct_builder_add_member(b, n, id, type.Raw, (uint)flags, hid);
        }
        GC.KeepAlive(type);
        if (rc != Status.OK)
        {
            Exception e = Errors.Of(rc);
            Ffi.rdds_struct_builder_release(b);
            b = 0;
            throw e;
        }
        return this;
    }

    public TypeRef Build()
    {
        nint h;
        int rc = Ffi.rdds_struct_builder_build(b, &h);
        Ffi.rdds_struct_builder_release(b);
        b = 0;
        return Types.Made(rc, h);
    }
}

/// <summary>Builds a union.</summary>
public sealed unsafe class UnionBuilder
{
    private nint b;

    /// <summary>discriminatorFlags: Key and the try-construct flags.</summary>
    public UnionBuilder(string name, Extensibility extensibility, TypeFlags flags, TypeRef discriminator,
        MemberFlags discriminatorFlags = MemberFlags.None)
    {
        Library.Check();
        nint h;
        int rc;
        fixed (byte* n = Utf8.Z(name))
        {
            rc = Ffi.rdds_union_builder_new(n, (int)extensibility, (uint)flags, discriminator.Raw,
                (uint)discriminatorFlags, &h);
        }
        GC.KeepAlive(discriminator);
        Errors.Check(rc);
        b = h;
    }

    /// <summary>A member with its labels (the integers of the discriminator
    /// values); the default member has MemberFlags.DefaultLabel.</summary>
    public UnionBuilder Member(string name, uint id, TypeRef type, int[] labels,
        MemberFlags flags = MemberFlags.None, string? hashId = null)
    {
        int rc;
        fixed (byte* n = Utf8.Z(name), hid = hashId is null ? null : Utf8.Z(hashId))
        fixed (int* l = labels)
        {
            rc = Ffi.rdds_union_builder_add_member(b, n, id, type.Raw, l, (nuint)labels.Length, (uint)flags, hid);
        }
        GC.KeepAlive(type);
        if (rc != Status.OK)
        {
            Exception e = Errors.Of(rc);
            Ffi.rdds_union_builder_release(b);
            b = 0;
            throw e;
        }
        return this;
    }

    public TypeRef Build()
    {
        nint h;
        int rc = Ffi.rdds_union_builder_build(b, &h);
        Ffi.rdds_union_builder_release(b);
        b = 0;
        return Types.Made(rc, h);
    }
}
