// Builds DDS-XTypes types through the C ABI (DDS-XTypes 1.3, 7.5.2.2), for
// the code rustorb-idl generates.
//
// Licensed under MIT OR Apache-2.0, like the rest of rustorb-dds.

package org.rustorb.dds;

import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

/**
 * The functions that build DDS-XTypes types, as the C ABI's
 * {@code rdds_type_*} and {@code rdds_*_builder_*} (see rustorb/dds.h).
 * Every function copies the types it is given.
 */
public final class Types {
    private Types() {}

    // Type kinds of the primitive types (DDS-XTypes 1.3, 7.3.4.6).
    public static final int TK_BOOLEAN = 0x01;
    public static final int TK_BYTE = 0x02;
    public static final int TK_INT16 = 0x03;
    public static final int TK_INT32 = 0x04;
    public static final int TK_INT64 = 0x05;
    public static final int TK_UINT16 = 0x06;
    public static final int TK_UINT32 = 0x07;
    public static final int TK_UINT64 = 0x08;
    public static final int TK_FLOAT32 = 0x09;
    public static final int TK_FLOAT64 = 0x0A;
    public static final int TK_FLOAT128 = 0x0B;
    public static final int TK_INT8 = 0x0C;
    public static final int TK_UINT8 = 0x0D;
    public static final int TK_CHAR8 = 0x10;
    public static final int TK_CHAR16 = 0x11;

    // Extensibility.
    public static final int FINAL = 0;
    public static final int APPENDABLE = 1;
    public static final int MUTABLE = 2;

    // Flags of a struct or union.
    public static final int TYPE_NESTED = 1;
    public static final int TYPE_AUTOID_HASH = 2;

    // Flags of a member.
    public static final int MEMBER_KEY = 1;
    public static final int MEMBER_OPTIONAL = 2;
    public static final int MEMBER_MUST_UNDERSTAND = 4;
    public static final int MEMBER_EXTERNAL = 8;
    public static final int MEMBER_DEFAULT_LABEL = 64;

    /** The version of the code rustorb-idl generates (the codecs) that
     * this rustorb-dds reads; java_dds.py of rustorb-idl has the same. */
    public static final int GENERATED_CODE_VERSION = 1;

    /** Called by each generated codec: IllegalStateException if it was
     * generated for another version of rustorb-dds. */
    public static void checkGeneratedCode(int version, String idl) {
        if (version != GENERATED_CODE_VERSION) {
            throw new IllegalStateException(idl + " was generated for another version of rustorb-dds (generated code "
                    + version + ", this rustorb-dds reads " + GENERATED_CODE_VERSION + "): run rustorb-idl again");
        }
    }

    private static TypeRef made(MemorySegment out, int rc) {
        Errors.check(null, rc);
        return new TypeRef(out.get(JAVA_LONG, 0));
    }

    private static long str(Arena arena, String s) {
        return s == null ? 0 : arena.allocateFrom(s).address();
    }

    public static TypeRef primitive(int kind) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_primitive.invokeExact((byte) kind, out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A string (bound 0: unbounded). */
    public static TypeRef string(int bound) {
        return string(false, bound);
    }

    /** A wstring (bound 0: unbounded). */
    public static TypeRef wstring(int bound) {
        return string(true, bound);
    }

    private static TypeRef string(boolean wide, int bound) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_string.invokeExact(wide, bound, out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A sequence (bound 0: unbounded). */
    public static TypeRef sequence(TypeRef element, int bound) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_sequence.invokeExact(element.handle(), bound, out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    public static TypeRef array(TypeRef element, int... dimensions) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment dims = arena.allocateFrom(JAVA_INT, dimensions);
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_array.invokeExact(element.handle(), dims.address(),
                    (long) dimensions.length, out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A map (bound 0: unbounded). */
    public static TypeRef map(TypeRef key, TypeRef element, int bound) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_map.invokeExact(key.handle(), element.handle(), bound,
                    out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** A typedef; the name is fully scoped ("A::B::T"). */
    public static TypeRef alias(String name, TypeRef related) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = arena.allocate(JAVA_LONG);
            return made(out, (int) Ffi.rdds_type_alias.invokeExact(str(arena, name), related.handle(),
                    out.address()));
        } catch (Throwable t) {
            throw Library.rethrow(t);
        }
    }

    /** Builds an enum: add the literals, then build. */
    public static final class EnumBuilder {
        private long handle;

        public EnumBuilder(String name, int bitBound, int extensibility) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_enum_builder_new.invokeExact(str(arena, name), (short) bitBound,
                        extensibility, out.address());
                Errors.check(null, rc);
                handle = out.get(JAVA_LONG, 0);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public EnumBuilder literal(String name, int value, boolean isDefault) {
            try (Arena arena = Arena.ofConfined()) {
                int rc = (int) Ffi.rdds_enum_builder_add_literal.invokeExact(handle, str(arena, name), value,
                        isDefault);
                Errors.check(null, rc);
                return this;
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public EnumBuilder literal(String name, int value) {
            return literal(name, value, false);
        }

        /** The type; the builder is freed. */
        public TypeRef build() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_enum_builder_build.invokeExact(handle, out.address());
                Ffi.rdds_enum_builder_release.invokeExact(handle);
                handle = 0;
                return made(out, rc);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    /** Builds a bitmask: add the flags, then build. */
    public static final class BitmaskBuilder {
        private long handle;

        public BitmaskBuilder(String name, int bitBound, int extensibility) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_bitmask_builder_new.invokeExact(str(arena, name), (short) bitBound,
                        extensibility, out.address());
                Errors.check(null, rc);
                handle = out.get(JAVA_LONG, 0);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public BitmaskBuilder flag(String name, int position) {
            try (Arena arena = Arena.ofConfined()) {
                int rc = (int) Ffi.rdds_bitmask_builder_add_flag.invokeExact(handle, str(arena, name),
                        (short) position);
                Errors.check(null, rc);
                return this;
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public TypeRef build() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_bitmask_builder_build.invokeExact(handle, out.address());
                Ffi.rdds_bitmask_builder_release.invokeExact(handle);
                handle = 0;
                return made(out, rc);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    /** Builds a struct: add the members, then build. */
    public static final class StructBuilder {
        private long handle;

        /** base: null for none. */
        public StructBuilder(String name, int extensibility, int flags, TypeRef base) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_struct_builder_new.invokeExact(str(arena, name), extensibility, flags,
                        base == null ? 0L : base.handle(), out.address());
                Errors.check(null, rc);
                handle = out.get(JAVA_LONG, 0);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        /** hashId: the string of @hashid ("" when none is given), or null. */
        public StructBuilder member(String name, int id, TypeRef type, int flags, String hashId) {
            try (Arena arena = Arena.ofConfined()) {
                int rc = (int) Ffi.rdds_struct_builder_add_member.invokeExact(handle, str(arena, name), id,
                        type.handle(), flags, str(arena, hashId));
                Errors.check(null, rc);
                return this;
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public TypeRef build() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_struct_builder_build.invokeExact(handle, out.address());
                Ffi.rdds_struct_builder_release.invokeExact(handle);
                handle = 0;
                return made(out, rc);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }

    /** Builds a union: add the members, then build. */
    public static final class UnionBuilder {
        private long handle;

        /** discriminatorFlags: MEMBER_KEY and the try-construct flags. */
        public UnionBuilder(String name, int extensibility, int flags, TypeRef discriminator,
                int discriminatorFlags) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_union_builder_new.invokeExact(str(arena, name), extensibility, flags,
                        discriminator.handle(), discriminatorFlags, out.address());
                Errors.check(null, rc);
                handle = out.get(JAVA_LONG, 0);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public UnionBuilder member(String name, int id, TypeRef type, int[] labels, int flags, String hashId) {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment l = arena.allocateFrom(JAVA_INT, labels);
                int rc = (int) Ffi.rdds_union_builder_add_member.invokeExact(handle, str(arena, name), id,
                        type.handle(), l.address(), (long) labels.length, flags, str(arena, hashId));
                Errors.check(null, rc);
                return this;
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }

        public TypeRef build() {
            try (Arena arena = Arena.ofConfined()) {
                MemorySegment out = arena.allocate(JAVA_LONG);
                int rc = (int) Ffi.rdds_union_builder_build.invokeExact(handle, out.address());
                Ffi.rdds_union_builder_release.invokeExact(handle);
                handle = 0;
                return made(out, rc);
            } catch (Throwable t) {
                throw Library.rethrow(t);
            }
        }
    }
}
