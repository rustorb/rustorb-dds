//! Topic types (`rdds_type`), built bottom-up: primitives, strings and
//! collections from their parts, and enums, bitmasks, structs and unions
//! with builders that take one literal or member at a time (DDS-XTypes
//! 1.3, 7.5.2.2 `DynamicTypeBuilder`).
//!
//! Every function that makes a type copies what it is given; the caller
//! keeps and releases its own handles.

use std::ffi::c_char;
use std::sync::Arc;

use rustorb_dds::DynamicType;
use rustorb_xtypes::type_object::{AppliedBuiltinMemberAnnotations, TypeIdentifier};
use rustorb_xtypes::types::*;

use crate::{Res, Status, bad, cstr, guard, obj, put, release, slice};

/// `rdds_type`.
pub type RddsType = DynamicType;

/// A builder of an enum (`rdds_enum_builder`).
#[derive(Debug)]
pub struct EnumBuilder(EnumType);

/// A builder of a bitmask (`rdds_bitmask_builder`).
#[derive(Debug)]
pub struct BitmaskBuilder(BitmaskType);

/// A builder of a struct (`rdds_struct_builder`).
#[derive(Debug)]
pub struct StructBuilder(StructType);

/// A builder of a union (`rdds_union_builder`).
#[derive(Debug)]
pub struct UnionBuilder(UnionType);

/// `RDDS_MEMBER_KEY`.
pub const MEMBER_KEY: u32 = 1;
/// `RDDS_MEMBER_OPTIONAL`.
pub const MEMBER_OPTIONAL: u32 = 2;
/// `RDDS_MEMBER_MUST_UNDERSTAND`.
pub const MEMBER_MUST_UNDERSTAND: u32 = 4;
/// `RDDS_MEMBER_EXTERNAL`.
pub const MEMBER_EXTERNAL: u32 = 8;
/// `RDDS_MEMBER_TRY_CONSTRUCT_USE_DEFAULT`.
pub const MEMBER_USE_DEFAULT: u32 = 16;
/// `RDDS_MEMBER_TRY_CONSTRUCT_TRIM`.
pub const MEMBER_TRIM: u32 = 32;
/// `RDDS_MEMBER_DEFAULT_LABEL`: the `default` branch of a union.
pub const MEMBER_DEFAULT_LABEL: u32 = 64;

/// `RDDS_TYPE_NESTED`.
pub const TYPE_NESTED: u32 = 1;
/// `RDDS_TYPE_AUTOID_HASH`.
pub const TYPE_AUTOID_HASH: u32 = 2;

const PRIMITIVES: [Primitive; 15] = [
    Primitive::Boolean,
    Primitive::Byte,
    Primitive::Int8,
    Primitive::Uint8,
    Primitive::Int16,
    Primitive::Uint16,
    Primitive::Int32,
    Primitive::Uint32,
    Primitive::Int64,
    Primitive::Uint64,
    Primitive::Float32,
    Primitive::Float64,
    Primitive::Float128,
    Primitive::Char8,
    Primitive::Char16,
];

fn extensibility(e: i32) -> Res<Extensibility> {
    Ok(match e {
        0 => Extensibility::Final,
        1 => Extensibility::Appendable,
        2 => Extensibility::Mutable,
        _ => return Err(bad(format!("unknown extensibility {e}"))),
    })
}

fn try_construct(flags: u32) -> TryConstruct {
    if flags & MEMBER_TRIM != 0 {
        TryConstruct::Trim
    } else if flags & MEMBER_USE_DEFAULT != 0 {
        TryConstruct::UseDefault
    } else {
        TryConstruct::Discard
    }
}

unsafe fn ty(p: *const RddsType) -> Res<Type> {
    // SAFETY: NULL or a live handle.
    Ok(unsafe { obj(p, "the type") }?.as_type().clone())
}

unsafe fn hash_id(p: *const c_char) -> Res<Option<String>> {
    if p.is_null() {
        Ok(None)
    } else {
        // SAFETY: a NUL-terminated string.
        Ok(Some(unsafe { cstr(p, "the hash id") }?.to_owned()))
    }
}

fn make(out: *mut *mut RddsType, t: Type) -> Res {
    // SAFETY: `out` is valid for a write (the header).
    unsafe { put(out, DynamicType::from(t)) }
}

/// `rdds_type_primitive`: the primitive type of a DDS-XTypes type kind
/// (`TK_BOOLEAN` ... `TK_CHAR16`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_primitive(kind: u8, out: *mut *mut RddsType) -> Status {
    guard(|| {
        let p = PRIMITIVES
            .iter()
            .find(|p| p.kind() == kind)
            .ok_or_else(|| bad(format!("{kind:#x} is not a primitive type kind")))?;
        make(out, Type::Primitive(*p))
    })
}

/// `rdds_type_string`: `string` or `wstring` (`wide`), bounded unless
/// `bound` is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_string(
    wide: bool,
    bound: u32,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| make(out, Type::String { wide, bound }))
}

/// `rdds_type_sequence`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_sequence(
    element: *const RddsType,
    bound: u32,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let element = Element::new(unsafe { ty(element) }?);
        make(
            out,
            Type::Sequence(Arc::new(SequenceType { element, bound })),
        )
    })
}

/// `rdds_type_array`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_array(
    element: *const RddsType,
    dimensions: *const u32,
    dimensions_len: usize,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let element = Element::new(unsafe { ty(element) }?);
        let dimensions = unsafe { slice(dimensions, dimensions_len) }?.to_vec();
        if dimensions.is_empty() || dimensions.contains(&0) {
            return Err(bad("an array needs dimensions, none of them 0"));
        }
        make(
            out,
            Type::Array(Arc::new(ArrayType {
                element,
                dimensions,
            })),
        )
    })
}

/// `rdds_type_map`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_map(
    key: *const RddsType,
    element: *const RddsType,
    bound: u32,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let key = Element::new(unsafe { ty(key) }?);
        let element = Element::new(unsafe { ty(element) }?);
        make(
            out,
            Type::Map(Arc::new(MapType {
                key,
                element,
                bound,
            })),
        )
    })
}

/// `rdds_type_alias`: a typedef.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_alias(
    name: *const c_char,
    related: *const RddsType,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let name = unsafe { cstr(name, "the name") }?.to_owned();
        let related = unsafe { ty(related) }?;
        make(out, Type::from(AliasType { name, related }))
    })
}

/// `rdds_enum_builder_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_enum_builder_new(
    name: *const c_char,
    bit_bound: u16,
    extensibility_kind: i32,
    out: *mut *mut EnumBuilder,
) -> Status {
    guard(|| {
        let e = EnumType {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            bit_bound,
            extensibility: extensibility(extensibility_kind)?,
            literals: Vec::new(),
        };
        unsafe { put(out, EnumBuilder(e)) }
    })
}

/// `rdds_enum_builder_add_literal`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_enum_builder_add_literal(
    builder: *mut EnumBuilder,
    name: *const c_char,
    value: i32,
    is_default: bool,
) -> Status {
    guard(|| {
        let b = unsafe { builder.as_mut() }.ok_or_else(|| bad("the builder is NULL"))?;
        b.0.literals.push(EnumLiteral {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            value,
            default: is_default,
        });
        Ok(())
    })
}

/// `rdds_enum_builder_build`: the enum; the builder is kept.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_enum_builder_build(
    builder: *const EnumBuilder,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let b = unsafe { obj(builder, "the builder") }?;
        make(out, Type::from(b.0.clone()))
    })
}

/// `rdds_enum_builder_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_enum_builder_release(builder: *mut EnumBuilder) {
    unsafe { release(builder) }
}

/// `rdds_bitmask_builder_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_bitmask_builder_new(
    name: *const c_char,
    bit_bound: u16,
    extensibility_kind: i32,
    out: *mut *mut BitmaskBuilder,
) -> Status {
    guard(|| {
        let b = BitmaskType {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            bit_bound,
            extensibility: extensibility(extensibility_kind)?,
            flags: Vec::new(),
        };
        unsafe { put(out, BitmaskBuilder(b)) }
    })
}

/// `rdds_bitmask_builder_add_flag`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_bitmask_builder_add_flag(
    builder: *mut BitmaskBuilder,
    name: *const c_char,
    position: u16,
) -> Status {
    guard(|| {
        let b = unsafe { builder.as_mut() }.ok_or_else(|| bad("the builder is NULL"))?;
        b.0.flags.push(Bitflag {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            position,
        });
        Ok(())
    })
}

/// `rdds_bitmask_builder_build`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_bitmask_builder_build(
    builder: *const BitmaskBuilder,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let b = unsafe { obj(builder, "the builder") }?;
        make(out, Type::from(b.0.clone()))
    })
}

/// `rdds_bitmask_builder_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_bitmask_builder_release(builder: *mut BitmaskBuilder) {
    unsafe { release(builder) }
}

/// `rdds_struct_builder_new`: `flags` of `RDDS_TYPE_NESTED` and
/// `RDDS_TYPE_AUTOID_HASH`; `base` NULL for none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_struct_builder_new(
    name: *const c_char,
    extensibility_kind: i32,
    flags: u32,
    base: *const RddsType,
    out: *mut *mut StructBuilder,
) -> Status {
    guard(|| {
        let s = StructType {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            extensibility: extensibility(extensibility_kind)?,
            nested: flags & TYPE_NESTED != 0,
            autoid_hash: flags & TYPE_AUTOID_HASH != 0,
            base: if base.is_null() {
                None
            } else {
                Some(unsafe { ty(base) }?)
            },
            members: Vec::new(),
        };
        unsafe { put(out, StructBuilder(s)) }
    })
}

/// `rdds_struct_builder_add_member`: `flags` of `RDDS_MEMBER_*`;
/// `hash_id` the string of `@hashid` (`""` for none given) or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_struct_builder_add_member(
    builder: *mut StructBuilder,
    name: *const c_char,
    id: u32,
    member_type: *const RddsType,
    flags: u32,
    hash_id_: *const c_char,
) -> Status {
    guard(|| {
        let b = unsafe { builder.as_mut() }.ok_or_else(|| bad("the builder is NULL"))?;
        let m = StructMember {
            key: flags & MEMBER_KEY != 0,
            optional: flags & MEMBER_OPTIONAL != 0,
            must_understand: flags & MEMBER_MUST_UNDERSTAND != 0,
            external: flags & MEMBER_EXTERNAL != 0,
            try_construct: try_construct(flags),
            annotations: AppliedBuiltinMemberAnnotations {
                hash_id: unsafe { hash_id(hash_id_) }?,
                ..Default::default()
            },
            ..StructMember::new(unsafe { cstr(name, "the name") }?, id, unsafe {
                ty(member_type)
            }?)
        };
        b.0.members.push(m);
        Ok(())
    })
}

/// `rdds_struct_builder_build`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_struct_builder_build(
    builder: *const StructBuilder,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let b = unsafe { obj(builder, "the builder") }?;
        make(out, Type::from(b.0.clone()))
    })
}

/// `rdds_struct_builder_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_struct_builder_release(builder: *mut StructBuilder) {
    unsafe { release(builder) }
}

/// `rdds_union_builder_new`: `discriminator_flags` may have
/// `RDDS_MEMBER_KEY` and the try-construct flags.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_union_builder_new(
    name: *const c_char,
    extensibility_kind: i32,
    flags: u32,
    discriminator: *const RddsType,
    discriminator_flags: u32,
    out: *mut *mut UnionBuilder,
) -> Status {
    guard(|| {
        let u = UnionType {
            name: unsafe { cstr(name, "the name") }?.to_owned(),
            extensibility: extensibility(extensibility_kind)?,
            nested: flags & TYPE_NESTED != 0,
            autoid_hash: flags & TYPE_AUTOID_HASH != 0,
            discriminator: unsafe { ty(discriminator) }?,
            discriminator_key: discriminator_flags & MEMBER_KEY != 0,
            discriminator_try_construct: try_construct(discriminator_flags),
            members: Vec::new(),
        };
        unsafe { put(out, UnionBuilder(u)) }
    })
}

/// `rdds_union_builder_add_member`: the case labels, and
/// `RDDS_MEMBER_DEFAULT_LABEL` in `flags` for the `default` branch.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_union_builder_add_member(
    builder: *mut UnionBuilder,
    name: *const c_char,
    id: u32,
    member_type: *const RddsType,
    labels: *const i32,
    labels_len: usize,
    flags: u32,
    hash_id_: *const c_char,
) -> Status {
    guard(|| {
        let b = unsafe { builder.as_mut() }.ok_or_else(|| bad("the builder is NULL"))?;
        let m = UnionMember {
            default: flags & MEMBER_DEFAULT_LABEL != 0,
            external: flags & MEMBER_EXTERNAL != 0,
            try_construct: try_construct(flags),
            annotations: AppliedBuiltinMemberAnnotations {
                hash_id: unsafe { hash_id(hash_id_) }?,
                ..Default::default()
            },
            ..UnionMember::new(
                unsafe { cstr(name, "the name") }?,
                id,
                unsafe { ty(member_type) }?,
                unsafe { slice(labels, labels_len) }?.to_vec(),
            )
        };
        b.0.members.push(m);
        Ok(())
    })
}

/// `rdds_union_builder_build`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_union_builder_build(
    builder: *const UnionBuilder,
    out: *mut *mut RddsType,
) -> Status {
    guard(|| {
        let b = unsafe { obj(builder, "the builder") }?;
        make(out, Type::from(b.0.clone()))
    })
}

/// `rdds_union_builder_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_union_builder_release(builder: *mut UnionBuilder) {
    unsafe { release(builder) }
}

/// `rdds_type_clone`: another handle of the same type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_clone(t: *const RddsType, out: *mut *mut RddsType) -> Status {
    guard(|| {
        let t = unsafe { obj(t, "the type") }?.clone();
        unsafe { put(out, t) }
    })
}

/// `rdds_type_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_release(t: *mut RddsType) {
    unsafe { release(t) }
}

/// `rdds_type_hashes`: the equivalence hashes (14 octets each) of the
/// minimal and complete TypeObjects of a type that can be a topic type
/// (DDS-XTypes 1.3, 7.3.4.9). They identify it on the network.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_hashes(
    t: *const RddsType,
    minimal: *mut u8,
    complete: *mut u8,
) -> Status {
    guard(|| {
        let t = unsafe { obj(t, "the type") }?;
        let objects = TypeObjects::new(t.as_type())?;
        for (id, dst) in [
            (objects.minimal_id(), minimal),
            (objects.complete_id(), complete),
        ] {
            let (TypeIdentifier::Minimal(h) | TypeIdentifier::Complete(h)) = id else {
                return Err(bad("the type has no hash: it is not a topic type"));
            };
            if dst.is_null() {
                return Err(bad("the output pointer is NULL"));
            }
            // SAFETY: room for 14 octets (the header).
            unsafe { std::ptr::copy_nonoverlapping(h.as_ptr(), dst, h.len()) };
        }
        Ok(())
    })
}
