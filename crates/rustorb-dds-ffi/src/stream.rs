//! Samples as streams of values in the order of their type.
//!
//! An output stream (`rdds_out`) takes the values of a sample: the
//! members of a struct in order (those of its base first), the length of
//! a sequence or map and then its elements (a map's as key, value, key,
//! value...), the elements of an array (all its dimensions, row by row),
//! whether an optional member is present and then its value if it is,
//! the discriminator of a union and then the value of the member it
//! selects (nothing when it selects none). Aliases and `@external` are
//! transparent. The elements of a sequence or array of a primitive type
//! other than `float128` go in one call (`rdds_out_*_array`) after the
//! length of a sequence. An input stream (`rdds_in`) gives the values of
//! a sample in the same order.
//!
//! The stream does not know the type: a writer matches the values to its
//! type when it writes them, and fails with `RETCODE_BAD_PARAMETER` on
//! values that do not fit. Reading a value of the wrong kind sets a
//! sticky error on the input stream (`rdds_in_status`) and returns zero.

use std::ffi::c_char;

use rustorb_cdr::LongDouble;
use rustorb_dds::{DynamicData, DynamicDataFactory, DynamicType, MEMBER_ID_INVALID};
use rustorb_xtypes::type_object::*;
use rustorb_xtypes::types::{Primitive, Type};

use crate::{Fail, OK, Res, Status, bad, guard, guard_value, put, release, set};

const ME: MemberId = MEMBER_ID_INVALID;

/// The elements of a sequence or array of a primitive type.
#[derive(Debug, Clone, PartialEq)]
pub enum Bulk {
    Boolean(Vec<bool>),
    Byte(Vec<u8>),
    Int8(Vec<i8>),
    Uint8(Vec<u8>),
    Int16(Vec<i16>),
    Uint16(Vec<u16>),
    Int32(Vec<i32>),
    Uint32(Vec<u32>),
    Int64(Vec<i64>),
    Uint64(Vec<u64>),
    Float32(Vec<f32>),
    Float64(Vec<f64>),
    Char8(Vec<u8>),
    Char16(Vec<u16>),
}

impl Bulk {
    fn len(&self) -> usize {
        match self {
            Bulk::Boolean(v) => v.len(),
            Bulk::Byte(v) | Bulk::Uint8(v) | Bulk::Char8(v) => v.len(),
            Bulk::Int8(v) => v.len(),
            Bulk::Int16(v) => v.len(),
            Bulk::Uint16(v) | Bulk::Char16(v) => v.len(),
            Bulk::Int32(v) => v.len(),
            Bulk::Uint32(v) => v.len(),
            Bulk::Int64(v) => v.len(),
            Bulk::Uint64(v) => v.len(),
            Bulk::Float32(v) => v.len(),
            Bulk::Float64(v) => v.len(),
        }
    }
}

/// One value of the stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Boolean(bool),
    Byte(u8),
    Int8(i8),
    Uint8(u8),
    Int16(i16),
    Uint16(u16),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Float32(f32),
    Float64(f64),
    Float128(LongDouble),
    Char8(u8),
    Char16(u16),
    String(String),
    WString(Vec<u16>),
    /// Of a sequence or map.
    Length(u32),
    /// Of an optional member.
    Present(bool),
    Discriminator(i64),
    Enum(i32),
    Bitmask(u64),
    Bulk(Bulk),
}

impl Value {
    fn name(&self) -> &'static str {
        match self {
            Value::Boolean(_) => "boolean",
            Value::Byte(_) => "octet",
            Value::Int8(_) => "int8",
            Value::Uint8(_) => "uint8",
            Value::Int16(_) => "int16",
            Value::Uint16(_) => "uint16",
            Value::Int32(_) => "int32",
            Value::Uint32(_) => "uint32",
            Value::Int64(_) => "int64",
            Value::Uint64(_) => "uint64",
            Value::Float32(_) => "float32",
            Value::Float64(_) => "float64",
            Value::Float128(_) => "float128",
            Value::Char8(_) => "char8",
            Value::Char16(_) => "char16",
            Value::String(_) => "string",
            Value::WString(_) => "wstring",
            Value::Length(_) => "length",
            Value::Present(_) => "presence",
            Value::Discriminator(_) => "discriminator",
            Value::Enum(_) => "enum",
            Value::Bitmask(_) => "bitmask",
            Value::Bulk(_) => "array of primitives",
        }
    }
}

/// `rdds_out`: the values of a sample being written.
#[derive(Debug, Default)]
pub struct Out {
    pub(crate) values: Vec<Value>,
    pub(crate) error: Option<String>,
}

/// `rdds_in`: the values of a sample being read.
#[derive(Debug, Default)]
pub struct In {
    values: Vec<Value>,
    next: usize,
    error: Option<String>,
}

fn resolve(mut ty: &Type) -> &Type {
    while let Type::Alias(a) = ty {
        ty = &a.related;
    }
    ty
}

/// The primitive of the elements of a sequence or array of `ty` that go
/// as a [`Bulk`], if they do.
fn bulk_element(ty: &Type) -> Option<Primitive> {
    let element = match resolve(ty) {
        Type::Sequence(s) => &s.element.ty,
        Type::Array(a) => &a.element.ty,
        _ => return None,
    };
    match resolve(element) {
        Type::Primitive(Primitive::Float128) => None,
        Type::Primitive(p) => Some(*p),
        _ => None,
    }
}

// ---------- values to DynamicData ------------------------------------------

struct Reader<'a> {
    values: &'a [Value],
    next: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, what: &str) -> Res<&'a Value> {
        let v = self
            .values
            .get(self.next)
            .ok_or_else(|| bad(format!("the sample ends where a {what} is expected")))?;
        self.next += 1;
        Ok(v)
    }
}

fn mismatch(expected: &str, got: &Value) -> Fail {
    bad(format!(
        "a {expected} is expected in the sample, not a {}",
        got.name()
    ))
}

fn char8(c: u8) -> char {
    char::from(c)
}

fn char16(c: u16) -> Res<char> {
    char::from_u32(c.into()).ok_or_else(|| bad(format!("{c:#x} is not a wchar")))
}

fn fill(d: &mut DynamicData, r: &mut Reader<'_>) -> Res {
    let ty = d.get_type();
    let kind = d.get_kind();
    macro_rules! one {
        ($variant:ident, $set:ident, $what:literal) => {{
            match r.take($what)? {
                Value::$variant(x) => d.$set(ME, *x)?,
                v => return Err(mismatch($what, v)),
            }
        }};
    }
    match kind {
        TK_BOOLEAN => one!(Boolean, set_boolean_value, "boolean"),
        TK_BYTE => match r.take("octet")? {
            Value::Byte(x) | Value::Uint8(x) => d.set_byte_value(ME, *x)?,
            v => return Err(mismatch("octet", v)),
        },
        TK_UINT8 => match r.take("uint8")? {
            Value::Byte(x) | Value::Uint8(x) => d.set_uint8_value(ME, *x)?,
            v => return Err(mismatch("uint8", v)),
        },
        TK_INT8 => one!(Int8, set_int8_value, "int8"),
        TK_INT16 => one!(Int16, set_int16_value, "int16"),
        TK_UINT16 => one!(Uint16, set_uint16_value, "uint16"),
        TK_INT32 => one!(Int32, set_int32_value, "int32"),
        TK_UINT32 => one!(Uint32, set_uint32_value, "uint32"),
        TK_INT64 => one!(Int64, set_int64_value, "int64"),
        TK_UINT64 => one!(Uint64, set_uint64_value, "uint64"),
        TK_FLOAT32 => one!(Float32, set_float32_value, "float32"),
        TK_FLOAT64 => one!(Float64, set_float64_value, "float64"),
        TK_FLOAT128 => one!(Float128, set_float128_value, "float128"),
        TK_CHAR8 => match r.take("char8")? {
            Value::Char8(c) => d.set_char8_value(ME, char8(*c))?,
            v => return Err(mismatch("char8", v)),
        },
        TK_CHAR16 => match r.take("char16")? {
            Value::Char16(c) => d.set_char16_value(ME, char16(*c)?)?,
            v => return Err(mismatch("char16", v)),
        },
        TK_STRING8 => match r.take("string")? {
            Value::String(s) => d.set_string_value(ME, s)?,
            v => return Err(mismatch("string", v)),
        },
        TK_STRING16 => match r.take("wstring")? {
            Value::WString(s) => {
                let s = String::from_utf16(s).map_err(|_| bad("a wstring is not UTF-16"))?;
                d.set_wstring_value(ME, &s)?
            }
            v => return Err(mismatch("wstring", v)),
        },
        TK_ENUM => match r.take("enum")? {
            // The narrowest integer that holds it, which any bit bound
            // takes.
            Value::Enum(x) => {
                if let Ok(x) = i8::try_from(*x) {
                    d.set_int8_value(ME, x)?
                } else if let Ok(x) = i16::try_from(*x) {
                    d.set_int16_value(ME, x)?
                } else {
                    d.set_int32_value(ME, *x)?
                }
            }
            v => return Err(mismatch("enum", v)),
        },
        TK_BITMASK => match r.take("bitmask")? {
            Value::Bitmask(x) => {
                let x = *x;
                if let Ok(x) = u8::try_from(x) {
                    d.set_uint8_value(ME, x)?
                } else if let Ok(x) = u16::try_from(x) {
                    d.set_uint16_value(ME, x)?
                } else if let Ok(x) = u32::try_from(x) {
                    d.set_uint32_value(ME, x)?
                } else {
                    d.set_uint64_value(ME, x)?
                }
            }
            v => return Err(mismatch("bitmask", v)),
        },
        TK_SEQUENCE | TK_ARRAY => {
            let sequence = kind == TK_SEQUENCE;
            let n = if sequence {
                match r.take("length")? {
                    Value::Length(n) => *n as usize,
                    v => return Err(mismatch("length", v)),
                }
            } else {
                d.get_item_count() as usize
            };
            if sequence {
                d.clear_all_values()?;
            }
            if bulk_element(ty.as_type()).is_some() {
                let b = match r.take("array of primitives")? {
                    Value::Bulk(b) => b,
                    v => return Err(mismatch("array of primitives", v)),
                };
                if b.len() != n {
                    return Err(bad(format!("{} elements where {n} are expected", b.len())));
                }
                set_bulk(d, b)?;
            } else {
                for i in 0..n {
                    fill(d.loan_value(i as MemberId)?, r)?;
                }
            }
        }
        TK_MAP => {
            let Type::Map(m) = resolve(ty.as_type()) else {
                return Err(bad("a map type is expected"));
            };
            let (kt, et) = (
                DynamicType::from(m.key.ty.clone()),
                DynamicType::from(m.element.ty.clone()),
            );
            let n = match r.take("length")? {
                Value::Length(n) => *n,
                v => return Err(mismatch("length", v)),
            };
            d.clear_all_values()?;
            for _ in 0..n {
                let mut k = DynamicDataFactory::create_data(&kt);
                fill(&mut k, r)?;
                let mut e = DynamicDataFactory::create_data(&et);
                fill(&mut e, r)?;
                d.map_insert(k, e)?;
            }
        }
        TK_STRUCTURE => {
            for m in ty.members() {
                if m.get_descriptor().is_optional {
                    match r.take("presence")? {
                        Value::Present(true) => {}
                        Value::Present(false) => {
                            d.clear_value(m.get_id())?;
                            continue;
                        }
                        v => return Err(mismatch("presence", v)),
                    }
                }
                fill(d.loan_value(m.get_id())?, r)?;
            }
        }
        TK_UNION => {
            match r.take("discriminator")? {
                Value::Discriminator(x) => d.set_discriminator_value(*x)?,
                v => return Err(mismatch("discriminator", v)),
            }
            if let Some(id) = d.selected_member() {
                fill(d.loan_value(id)?, r)?;
            }
        }
        k => return Err(bad(format!("type kind {k:#x} is not supported"))),
    }
    Ok(())
}

fn set_bulk(d: &mut DynamicData, b: &Bulk) -> Res {
    match (d.get_kind_of_elements(), b) {
        (TK_BOOLEAN, Bulk::Boolean(v)) => d.set_boolean_values(ME, v)?,
        (TK_BYTE, Bulk::Byte(v) | Bulk::Uint8(v)) => d.set_byte_values(ME, v)?,
        (TK_UINT8, Bulk::Byte(v) | Bulk::Uint8(v)) => d.set_uint8_values(ME, v)?,
        (TK_INT8, Bulk::Int8(v)) => d.set_int8_values(ME, v)?,
        (TK_INT16, Bulk::Int16(v)) => d.set_int16_values(ME, v)?,
        (TK_UINT16, Bulk::Uint16(v)) => d.set_uint16_values(ME, v)?,
        (TK_INT32, Bulk::Int32(v)) => d.set_int32_values(ME, v)?,
        (TK_UINT32, Bulk::Uint32(v)) => d.set_uint32_values(ME, v)?,
        (TK_INT64, Bulk::Int64(v)) => d.set_int64_values(ME, v)?,
        (TK_UINT64, Bulk::Uint64(v)) => d.set_uint64_values(ME, v)?,
        (TK_FLOAT32, Bulk::Float32(v)) => d.set_float32_values(ME, v)?,
        (TK_FLOAT64, Bulk::Float64(v)) => d.set_float64_values(ME, v)?,
        (TK_CHAR8, Bulk::Char8(v)) => {
            let v: Vec<char> = v.iter().map(|c| char8(*c)).collect();
            d.set_char8_values(ME, &v)?
        }
        (TK_CHAR16, Bulk::Char16(v)) => {
            let v = v.iter().map(|c| char16(*c)).collect::<Res<Vec<char>>>()?;
            d.set_char16_values(ME, &v)?
        }
        (k, b) => {
            return Err(bad(format!(
                "an array of {} elements does not fit elements of kind {k:#x}",
                Value::Bulk(b.clone()).name()
            )));
        }
    }
    Ok(())
}

/// A sample of `ty` from the values of `out`.
pub(crate) fn to_data(ty: &DynamicType, out: &Out) -> Res<DynamicData> {
    if let Some(e) = &out.error {
        return Err(bad(e.clone()));
    }
    let mut d = DynamicDataFactory::create_data(ty);
    let mut r = Reader {
        values: &out.values,
        next: 0,
    };
    fill(&mut d, &mut r)?;
    if r.next != out.values.len() {
        return Err(bad(format!(
            "{} values after the end of the sample",
            out.values.len() - r.next
        )));
    }
    Ok(d)
}

// ---------- DynamicData to values ------------------------------------------

fn emit(d: &DynamicData, out: &mut Vec<Value>) -> Res {
    let ty = d.get_type();
    let kind = d.get_kind();
    macro_rules! one {
        ($variant:ident, $get:ident) => {
            out.push(Value::$variant(d.$get(ME)?))
        };
    }
    match kind {
        TK_BOOLEAN => one!(Boolean, get_boolean_value),
        TK_BYTE => one!(Byte, get_byte_value),
        TK_UINT8 => one!(Uint8, get_uint8_value),
        TK_INT8 => one!(Int8, get_int8_value),
        TK_INT16 => one!(Int16, get_int16_value),
        TK_UINT16 => one!(Uint16, get_uint16_value),
        TK_INT32 => one!(Int32, get_int32_value),
        TK_UINT32 => one!(Uint32, get_uint32_value),
        TK_INT64 => one!(Int64, get_int64_value),
        TK_UINT64 => one!(Uint64, get_uint64_value),
        TK_FLOAT32 => one!(Float32, get_float32_value),
        TK_FLOAT64 => one!(Float64, get_float64_value),
        TK_FLOAT128 => one!(Float128, get_float128_value),
        TK_CHAR8 => out.push(Value::Char8(u32::from(d.get_char8_value(ME)?) as u8)),
        TK_CHAR16 => out.push(Value::Char16(u32::from(d.get_char16_value(ME)?) as u16)),
        TK_STRING8 => out.push(Value::String(d.get_string_value(ME)?)),
        TK_STRING16 => out.push(Value::WString(
            d.get_wstring_value(ME)?.encode_utf16().collect(),
        )),
        TK_ENUM => out.push(Value::Enum(d.get_int32_value(ME)?)),
        TK_BITMASK => out.push(Value::Bitmask(d.get_uint64_value(ME)?)),
        TK_SEQUENCE | TK_ARRAY => {
            let n = d.get_item_count();
            if kind == TK_SEQUENCE {
                out.push(Value::Length(n));
            }
            if bulk_element(ty.as_type()).is_some() {
                out.push(Value::Bulk(get_bulk(d)?));
            } else {
                for i in 0..n {
                    emit(d.get_value(i)?, out)?;
                }
            }
        }
        TK_MAP => {
            let entries = d.map_entries()?;
            out.push(Value::Length(entries.len() as u32));
            for (k, v) in entries {
                emit(k, out)?;
                emit(v, out)?;
            }
        }
        TK_STRUCTURE => {
            for m in ty.members() {
                let optional = m.get_descriptor().is_optional;
                if optional {
                    let present = d.is_present(m.get_id());
                    out.push(Value::Present(present));
                    if !present {
                        continue;
                    }
                }
                emit(d.get_value(m.get_id())?, out)?;
            }
        }
        TK_UNION => {
            out.push(Value::Discriminator(d.get_discriminator_value()?));
            if let Some(id) = d.selected_member() {
                emit(d.get_value(id)?, out)?;
            }
        }
        k => return Err(bad(format!("type kind {k:#x} is not supported"))),
    }
    Ok(())
}

fn get_bulk(d: &DynamicData) -> Res<Bulk> {
    Ok(match d.get_kind_of_elements() {
        TK_BOOLEAN => Bulk::Boolean(d.get_boolean_values(ME)?),
        TK_BYTE => Bulk::Byte(d.get_byte_values(ME)?),
        TK_UINT8 => Bulk::Uint8(d.get_uint8_values(ME)?),
        TK_INT8 => Bulk::Int8(d.get_int8_values(ME)?),
        TK_INT16 => Bulk::Int16(d.get_int16_values(ME)?),
        TK_UINT16 => Bulk::Uint16(d.get_uint16_values(ME)?),
        TK_INT32 => Bulk::Int32(d.get_int32_values(ME)?),
        TK_UINT32 => Bulk::Uint32(d.get_uint32_values(ME)?),
        TK_INT64 => Bulk::Int64(d.get_int64_values(ME)?),
        TK_UINT64 => Bulk::Uint64(d.get_uint64_values(ME)?),
        TK_FLOAT32 => Bulk::Float32(d.get_float32_values(ME)?),
        TK_FLOAT64 => Bulk::Float64(d.get_float64_values(ME)?),
        TK_CHAR8 => Bulk::Char8(
            d.get_char8_values(ME)?
                .into_iter()
                .map(|c| u32::from(c) as u8)
                .collect(),
        ),
        TK_CHAR16 => Bulk::Char16(
            d.get_char16_values(ME)?
                .into_iter()
                .map(|c| u32::from(c) as u16)
                .collect(),
        ),
        k => return Err(bad(format!("elements of kind {k:#x} are not primitives"))),
    })
}

/// An input stream of the values of `d`.
pub(crate) fn from_data(d: &DynamicData) -> Res<In> {
    let mut values = Vec::new();
    emit(d, &mut values)?;
    Ok(In {
        values,
        next: 0,
        error: None,
    })
}

trait ElementKind {
    fn get_kind_of_elements(&self) -> TypeKind;
}

impl ElementKind for DynamicData {
    /// The kind of the elements of a sequence or array, aliases resolved.
    fn get_kind_of_elements(&self) -> TypeKind {
        bulk_element(self.get_type().as_type()).map_or(TK_NONE, Primitive::kind)
    }
}

// ---------- the C functions: output ----------------------------------------

/// `rdds_out_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_new(out: *mut *mut Out) -> Status {
    guard(|| unsafe { put(out, Out::default()) })
}

/// `rdds_out_clear`: empties the stream to write another sample.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_clear(s: *mut Out) {
    if let Some(s) = unsafe { s.as_mut() } {
        s.values.clear();
        s.error = None;
    }
}

/// `rdds_out_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_release(s: *mut Out) {
    unsafe { release(s) }
}

fn push(s: *mut Out, v: impl FnOnce() -> Result<Value, String>) {
    let Some(s) = (unsafe { s.as_mut() }) else {
        return;
    };
    guard_value((), || {
        if s.error.is_some() {
            return;
        }
        match v() {
            Ok(v) => s.values.push(v),
            Err(e) => s.error = Some(e),
        }
    })
}

macro_rules! out_scalar {
    ($($name:ident, $t:ty, $variant:ident;)*) => {$(
        #[doc = concat!("`", stringify!($name), "`.")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *mut Out, v: $t) {
            push(s, || Ok(Value::$variant(v)))
        }
    )*};
}

out_scalar! {
    rdds_out_boolean, bool, Boolean;
    rdds_out_octet, u8, Byte;
    rdds_out_int8, i8, Int8;
    rdds_out_uint8, u8, Uint8;
    rdds_out_int16, i16, Int16;
    rdds_out_uint16, u16, Uint16;
    rdds_out_int32, i32, Int32;
    rdds_out_uint32, u32, Uint32;
    rdds_out_int64, i64, Int64;
    rdds_out_uint64, u64, Uint64;
    rdds_out_float32, f32, Float32;
    rdds_out_float64, f64, Float64;
    rdds_out_char8, u8, Char8;
    rdds_out_char16, u16, Char16;
    rdds_out_length, u32, Length;
    rdds_out_present, bool, Present;
    rdds_out_discriminator, i64, Discriminator;
    rdds_out_enum, i32, Enum;
    rdds_out_bitmask, u64, Bitmask;
}

/// `rdds_out_float128`: the IEEE 754 binary128 bits, high and low halves.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_float128(s: *mut Out, high: u64, low: u64) {
    push(s, || {
        Ok(Value::Float128(LongDouble::from_bits(
            (u128::from(high) << 64) | u128::from(low),
        )))
    })
}

/// `rdds_out_string`: `len` octets of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_string(s: *mut Out, v: *const c_char, len: usize) {
    push(s, || {
        let bytes = unsafe { crate::slice(v.cast::<u8>(), len) }.map_err(|f| f.1)?;
        String::from_utf8(bytes.to_vec())
            .map(Value::String)
            .map_err(|_| "a string is not UTF-8".to_owned())
    })
}

/// `rdds_out_wstring`: `len` UTF-16 code units.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_out_wstring(s: *mut Out, v: *const u16, len: usize) {
    push(s, || {
        let units = unsafe { crate::slice(v, len) }.map_err(|f| f.1)?;
        Ok(Value::WString(units.to_vec()))
    })
}

macro_rules! out_array {
    ($($name:ident, $t:ty, $variant:ident;)*) => {$(
        #[doc = concat!("`", stringify!($name), "`: `n` elements.")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *mut Out, v: *const $t, n: usize) {
            push(s, || {
                let v = unsafe { crate::slice(v, n) }.map_err(|f| f.1)?;
                Ok(Value::Bulk(Bulk::$variant(v.to_vec())))
            })
        }
    )*};
}

out_array! {
    rdds_out_boolean_array, bool, Boolean;
    rdds_out_octet_array, u8, Byte;
    rdds_out_int8_array, i8, Int8;
    rdds_out_uint8_array, u8, Uint8;
    rdds_out_int16_array, i16, Int16;
    rdds_out_uint16_array, u16, Uint16;
    rdds_out_int32_array, i32, Int32;
    rdds_out_uint32_array, u32, Uint32;
    rdds_out_int64_array, i64, Int64;
    rdds_out_uint64_array, u64, Uint64;
    rdds_out_float32_array, f32, Float32;
    rdds_out_float64_array, f64, Float64;
    rdds_out_char8_array, u8, Char8;
    rdds_out_char16_array, u16, Char16;
}

// ---------- the C functions: input -----------------------------------------

/// `rdds_in_status`: `RETCODE_OK`, or `RETCODE_BAD_PARAMETER` after a
/// read that did not fit the sample (and its message,
/// `rdds_last_error_message`, is set).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_status(s: *const In) -> Status {
    match unsafe { s.as_ref() } {
        Some(In { error: None, .. }) => OK,
        Some(In { error: Some(e), .. }) => guard(|| Err(bad(e.clone()))),
        None => guard(|| Err(bad("the stream is NULL"))),
    }
}

/// `rdds_in_at_end`: whether every value was read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_at_end(s: *const In) -> bool {
    unsafe { s.as_ref() }.is_none_or(|s| s.next >= s.values.len())
}

/// `rdds_in_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_release(s: *mut In) {
    unsafe { release(s) }
}

/// The next value if `f` takes it; otherwise the error is set and None.
fn pull<'a, T>(s: *mut In, what: &str, f: impl FnOnce(&'a Value) -> Option<T>) -> Option<T> {
    let s: &'a mut In = unsafe { s.as_mut() }?;
    if s.error.is_some() {
        return None;
    }
    let got = s.values.get(s.next);
    match got.and_then(f) {
        Some(x) => {
            s.next += 1;
            Some(x)
        }
        None => {
            s.error = Some(match got {
                Some(v) => format!("a {what} was read where the sample has a {}", v.name()),
                None => format!("a {what} was read after the end of the sample"),
            });
            None
        }
    }
}

macro_rules! in_scalar {
    ($($name:ident, $t:ty, $what:literal, $($pat:pat => $e:expr),+;)*) => {$(
        #[doc = concat!("`", stringify!($name), "`.")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *mut In) -> $t {
            guard_value(<$t>::default(), || {
                pull(s, $what, |v| match v { $($pat => Some($e),)+ _ => None })
                    .unwrap_or_default()
            })
        }
    )*};
}

in_scalar! {
    rdds_in_boolean, bool, "boolean", Value::Boolean(x) => *x;
    rdds_in_octet, u8, "octet", Value::Byte(x) | Value::Uint8(x) => *x;
    rdds_in_int8, i8, "int8", Value::Int8(x) => *x;
    rdds_in_uint8, u8, "uint8", Value::Uint8(x) | Value::Byte(x) => *x;
    rdds_in_int16, i16, "int16", Value::Int16(x) => *x;
    rdds_in_uint16, u16, "uint16", Value::Uint16(x) => *x;
    rdds_in_int32, i32, "int32", Value::Int32(x) => *x;
    rdds_in_uint32, u32, "uint32", Value::Uint32(x) => *x;
    rdds_in_int64, i64, "int64", Value::Int64(x) => *x;
    rdds_in_uint64, u64, "uint64", Value::Uint64(x) => *x;
    rdds_in_float32, f32, "float32", Value::Float32(x) => *x;
    rdds_in_float64, f64, "float64", Value::Float64(x) => *x;
    rdds_in_char8, u8, "char8", Value::Char8(x) => *x;
    rdds_in_char16, u16, "char16", Value::Char16(x) => *x;
    rdds_in_length, u32, "length", Value::Length(x) => *x;
    rdds_in_present, bool, "presence", Value::Present(x) => *x;
    rdds_in_discriminator, i64, "discriminator", Value::Discriminator(x) => *x;
    rdds_in_enum, i32, "enum", Value::Enum(x) => *x;
    rdds_in_bitmask, u64, "bitmask", Value::Bitmask(x) => *x;
}

/// `rdds_in_float128`: the binary128 bits into `*high` and `*low`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_float128(s: *mut In, high: *mut u64, low: *mut u64) {
    guard_value((), || {
        let bits = pull(s, "float128", |v| match v {
            Value::Float128(x) => Some(x.to_bits()),
            _ => None,
        })
        .unwrap_or(0);
        let _ = unsafe { set(high, (bits >> 64) as u64) };
        let _ = unsafe { set(low, bits as u64) };
    })
}

/// `rdds_in_string`: the UTF-8 octets of a string (not NUL-terminated),
/// valid until the stream is released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_string(s: *mut In, v: *mut *const c_char, len: *mut usize) {
    guard_value((), || {
        let (p, n) = pull(s, "string", |x| match x {
            Value::String(x) => Some((x.as_ptr().cast::<c_char>(), x.len())),
            _ => None,
        })
        .unwrap_or((c"".as_ptr(), 0));
        let _ = unsafe { set(v, p) };
        let _ = unsafe { set(len, n) };
    })
}

/// `rdds_in_wstring`: the UTF-16 code units of a wstring, valid until
/// the stream is released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_in_wstring(s: *mut In, v: *mut *const u16, len: *mut usize) {
    guard_value((), || {
        let (p, n) = pull(s, "wstring", |x| match x {
            Value::WString(x) => Some((x.as_ptr(), x.len())),
            _ => None,
        })
        .unwrap_or((std::ptr::null(), 0));
        let _ = unsafe { set(v, p) };
        let _ = unsafe { set(len, n) };
    })
}

macro_rules! in_array {
    ($($name:ident, $t:ty, $what:literal, $($variant:ident)|+;)*) => {$(
        #[doc = concat!("`", stringify!($name), "`: exactly `n` elements into `v`.")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(s: *mut In, v: *mut $t, n: usize) {
            guard_value((), || {
                let Some(src) = pull(s, $what, |x| match x {
                    $(Value::Bulk(Bulk::$variant(x)))|+ if x.len() == n => Some(x.as_slice()),
                    _ => None,
                }) else {
                    return;
                };
                if n > 0 && !v.is_null() {
                    // SAFETY: room for `n` elements (the header).
                    unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), v, n) };
                }
            })
        }
    )*};
}

in_array! {
    rdds_in_boolean_array, bool, "array of booleans", Boolean;
    rdds_in_octet_array, u8, "array of octets", Byte | Uint8;
    rdds_in_int8_array, i8, "array of int8", Int8;
    rdds_in_uint8_array, u8, "array of uint8", Uint8 | Byte;
    rdds_in_int16_array, i16, "array of int16", Int16;
    rdds_in_uint16_array, u16, "array of uint16", Uint16;
    rdds_in_int32_array, i32, "array of int32", Int32;
    rdds_in_uint32_array, u32, "array of uint32", Uint32;
    rdds_in_int64_array, i64, "array of int64", Int64;
    rdds_in_uint64_array, u64, "array of uint64", Uint64;
    rdds_in_float32_array, f32, "array of float32", Float32;
    rdds_in_float64_array, f64, "array of float64", Float64;
    rdds_in_char8_array, u8, "array of char8", Char8;
    rdds_in_char16_array, u16, "array of char16", Char16;
}

/// `rdds_type_serialize`: the serialized payload of a sample (with its
/// encapsulation header), in XCDR1 (`xcdr2` false) or XCDR2, little
/// endian; `*data` is freed with `rdds_bytes_free`. For tests and tools.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_serialize(
    t: *const DynamicType,
    sample: *const Out,
    xcdr2: bool,
    data: *mut *mut u8,
    len: *mut usize,
) -> Status {
    guard(|| {
        let t = unsafe { crate::obj(t, "the type") }?;
        let sample = unsafe { crate::obj(sample, "the sample") }?;
        let d = to_data(t, sample)?;
        let enc = if xcdr2 {
            rustorb_cdr::Encoding::Xcdr2
        } else {
            rustorb_cdr::Encoding::Xcdr1
        };
        let bytes = d.to_payload(enc, rustorb_cdr::ByteOrder::Little)?;
        let bytes = bytes.into_boxed_slice();
        unsafe { set(len, bytes.len()) }?;
        unsafe { set(data, Box::into_raw(bytes).cast::<u8>()) }
    })
}

/// `rdds_type_deserialize`: a sample from its serialized payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_type_deserialize(
    t: *const DynamicType,
    data: *const u8,
    len: usize,
    out: *mut *mut In,
) -> Status {
    guard(|| {
        let t = unsafe { crate::obj(t, "the type") }?;
        let bytes = unsafe { crate::slice(data, len) }?;
        let d = DynamicData::from_payload(t, bytes)?;
        unsafe { put(out, from_data(&d)?) }
    })
}

/// `rdds_bytes_free`: frees what `rdds_type_serialize` made.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rdds_bytes_free(data: *mut u8, len: usize) {
    if !data.is_null() {
        // SAFETY: a boxed slice of `len` octets made by this library.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, len)) });
    }
}
