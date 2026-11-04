//! The XCDR1 and XCDR2 serialization of [`DynamicData`] (7.4.3.5.3, rules
//! 1 to 30), its key holder and its key hash (7.6.8).
//!
//! The octets are those of the code rustorb-idl generates for the same
//! type (rustorb-typesupport), so a value written by one is read by the
//! other. Where the specification leaves something open:
//!
//! - A map of a non-primitive key or element type in XCDR2 (rule 15)
//!   has a DHEADER, then its length like the sequences of rule 12.
//! - The discriminator of a mutable union is the first member, written
//!   with member ID 0 and the must-understand flag (rules 27 and 28); a
//!   reader takes the first member as the discriminator, whatever its ID.

use rustorb_cdr::xcdr::{DHeaderMark, Delimited, ListEnd, MemberHeader, Parameter, XcdrFormat};
use rustorb_cdr::{ByteOrder, Decoder, Encoder, Encoding};

use super::data::{DynamicData, Value, array_len, discriminator_value, selected};
use super::ty::{DynamicType, resolve, struct_members};
use crate::type_object::MemberId;
use crate::types::*;
use crate::{Error, Result};

impl DynamicData {
    /// Writes the value to `enc`, in the encoding of `enc` (XCDR1 or
    /// XCDR2), without an encapsulation header.
    pub fn serialize(&self, enc: &mut Encoder) -> Result<()> {
        write(self, enc, false)
    }

    /// Reads a value of `ty` from `dec`, in the encoding of `dec`.
    pub fn deserialize(ty: &DynamicType, dec: &mut Decoder<'_>) -> Result<DynamicData> {
        read(ty.as_type(), dec)
    }

    /// The RTPS SerializedPayload of the value: the encapsulation header
    /// of `encoding` (XCDR1 or XCDR2) and the extensibility of the type,
    /// the value, and the padding to 4 octets (rule 1, DDSI-RTPS 10).
    pub fn to_payload(&self, encoding: Encoding, order: ByteOrder) -> Result<Vec<u8>> {
        let format = XcdrFormat::new(encoding, extensibility_of(&self.ty))
            .ok_or(Error::Unsupported("an encoding other than XCDR1 and XCDR2"))?;
        let mut enc = Encoder::xcdr_payload(format, order);
        write(self, &mut enc, false)?;
        Ok(enc.finish_xcdr_payload())
    }

    /// Reads a value of `ty` from an RTPS SerializedPayload, whose format
    /// must be the one of the extensibility of `ty` (PL_CDR or PL_CDR2 for
    /// a mutable type, PLAIN_CDR or DELIMITED_CDR for an appendable one,
    /// PLAIN_CDR or PLAIN_CDR2 for a final one).
    pub fn from_payload(ty: &DynamicType, buf: &[u8]) -> Result<DynamicData> {
        let (mut dec, header) = Decoder::xcdr_payload(buf)?;
        let format = header.id.format;
        if XcdrFormat::new(format.encoding(), extensibility_of(ty.as_type())) != Some(format) {
            return Err(Error::Invalid(
                "a payload format that is not the one of the extensibility of the type",
            ));
        }
        read(ty.as_type(), &mut dec)
    }

    /// The PLAIN_CDR2 big endian serialization of the key holder of the
    /// value (7.6.8, steps 1 to 4): a final type with the key members (all
    /// the members when there are none) in the order of their member IDs,
    /// each written as its own key holder.
    pub fn key_holder(&self) -> Result<Vec<u8>> {
        let mut enc = Encoder::new(ByteOrder::Big, Encoding::Xcdr2);
        write(self, &mut enc, true)?;
        Ok(enc.into_bytes())
    }

    /// The key hash of the value (7.6.8, step 5): the key holder padded
    /// with zeros when the largest one of the type fits in 16 octets, its
    /// MD5 otherwise.
    pub fn key_hash(&self) -> Result<[u8; 16]> {
        let holder = self.key_holder()?;
        match max_key_end(&self.ty, 0) {
            Some(max) if max <= 16 => {
                let mut hash = [0u8; 16];
                hash[..holder.len()].copy_from_slice(&holder);
                Ok(hash)
            }
            _ => Ok(rustorb_typesupport::md5(&holder)),
        }
    }
}

/// The extensibility kind of a type; final for the types that have none.
pub(crate) fn extensibility_of(ty: &Type) -> Extensibility {
    match resolve(ty) {
        Type::Struct(s) => s.extensibility,
        Type::Union(u) => u.extensibility,
        Type::Enum(e) => e.extensibility,
        Type::Bitmask(b) => b.extensibility,
        _ => Extensibility::Final,
    }
}

fn invalid(what: &'static str) -> Error {
    Error::Invalid(what)
}

fn is_v2(enc: &Encoder) -> bool {
    enc.encoding() == Encoding::Xcdr2
}

fn is_v2_in(dec: &Decoder<'_>) -> bool {
    dec.encoding() == Encoding::Xcdr2
}

/// Whether `ty` is a primitive type (7.2.2.2): collections of anything
/// else have a DHEADER in XCDR2 (rules 9, 12 and 15). Enums and bitmasks
/// are not primitive.
fn is_primitive(ty: &Type) -> bool {
    matches!(resolve(ty), Type::Primitive(_))
}

/// The element type of an array, arrays of arrays being one array (Table
/// 40, ARRAY_TYPE).
fn innermost(a: &ArrayType) -> &Type {
    let mut ty = resolve(&a.element.ty);
    while let Type::Array(inner) = ty {
        ty = resolve(&inner.element.ty);
    }
    ty
}

/// The size of the holder type of an enum (7.3.1.2.1.5 `@bit_bound`).
fn enum_holder_size(bit_bound: u16) -> usize {
    match bit_bound {
        0..=8 => 1,
        9..=16 => 2,
        _ => 4,
    }
}

/// The size of the holder type of a bitmask.
fn bitmask_holder_size(bit_bound: u16) -> usize {
    match bit_bound {
        0..=8 => 1,
        9..=16 => 2,
        17..=32 => 4,
        _ => 8,
    }
}

fn check_string(s: &str, wide: bool, bound: u32) -> Result<()> {
    let len = if wide {
        s.encode_utf16().count()
    } else {
        s.len()
    };
    if bound != 0 && len > bound as usize {
        return Err(invalid("a string longer than its bound"));
    }
    Ok(())
}

fn check_bound(len: usize, bound: u32) -> Result<()> {
    if bound != 0 && len > bound as usize {
        return Err(invalid("a collection longer than its bound"));
    }
    Ok(())
}

// ---------- writing -----------------------------------------------------

/// An aggregated value being written: its DHEADER (XCDR2, appendable and
/// mutable) or its parameter list (XCDR1, mutable).
struct Out {
    dheader: Option<DHeaderMark>,
    parameter_list: bool,
}

fn begin(enc: &mut Encoder, kind: Extensibility) -> Out {
    let v2 = is_v2(enc);
    Out {
        dheader: (v2 && kind != Extensibility::Final).then(|| enc.begin_dheader()),
        parameter_list: !v2 && kind == Extensibility::Mutable,
    }
}

fn end(enc: &mut Encoder, out: Out) -> Result<()> {
    if out.parameter_list {
        enc.end_parameter_list(ListEnd::Xtypes);
    }
    if let Some(mark) = out.dheader {
        enc.end_dheader(mark)?;
    }
    Ok(())
}

/// Writes `v` as member `id` of a mutable type: its EMHEADER (XCDR2, rule
/// 22) or parameter header (XCDR1, rules 24 and 25), then the value.
fn write_member(
    enc: &mut Encoder,
    id: MemberId,
    must_understand: bool,
    v: &DynamicData,
) -> Result<()> {
    if is_v2(enc) {
        let mark = enc.begin_member(id, must_understand)?;
        write(v, enc, false)?;
        enc.end_member(mark)?;
    } else {
        let mark = enc.begin_parameter(id, must_understand)?;
        write(v, enc, false)?;
        enc.end_parameter(mark)?;
    }
    Ok(())
}

/// Ends a DHEADER when there is one.
fn end_dheader(enc: &mut Encoder, mark: Option<DHeaderMark>) -> Result<()> {
    if let Some(mark) = mark {
        enc.end_dheader(mark)?;
    }
    Ok(())
}

/// Writes `d`, as its key holder when `key`.
fn write(d: &DynamicData, enc: &mut Encoder, key: bool) -> Result<()> {
    match (&d.ty, &d.value) {
        (_, Value::Boolean(b)) => enc.write_bool(*b),
        (_, Value::Byte(x) | Value::Uint8(x)) => enc.write_octet(*x),
        (_, Value::Int8(x)) => enc.write_octet(*x as u8),
        (_, Value::Int16(x)) => enc.write_i16(*x),
        (_, Value::Uint16(x)) => enc.write_u16(*x),
        (_, Value::Int32(x)) => enc.write_i32(*x),
        (_, Value::Uint32(x)) => enc.write_u32(*x),
        (_, Value::Int64(x)) => enc.write_i64(*x),
        (_, Value::Uint64(x)) => enc.write_u64(*x),
        (_, Value::Float32(x)) => enc.write_f32(*x),
        (_, Value::Float64(x)) => enc.write_f64(*x),
        (_, Value::Float128(x)) => enc.write_long_double(*x),
        (_, Value::Char8(c)) => {
            let b = u8::try_from(u32::from(*c)).map_err(|_| invalid("a char above U+00FF"))?;
            enc.write_char(b);
        }
        (_, Value::Char16(c)) => {
            let unit = u16::try_from(u32::from(*c)).map_err(|_| invalid("a wchar above U+FFFF"))?;
            enc.write_wchar(unit)?;
        }
        (Type::String { wide, bound }, Value::String(s)) => {
            check_string(s, *wide, *bound)?;
            if *wide {
                enc.write_wstring(s)?;
            } else {
                enc.write_string(s)?;
            }
        }
        (Type::Enum(e), Value::Enum(v)) => match enum_holder_size(e.bit_bound) {
            1 => enc.write_octet(*v as i8 as u8),
            2 => enc.write_i16(*v as i16),
            _ => enc.write_i32(*v),
        },
        (Type::Bitmask(b), Value::Bitmask(v)) => match bitmask_holder_size(b.bit_bound) {
            1 => enc.write_octet(*v as u8),
            2 => enc.write_u16(*v as u16),
            4 => enc.write_u32(*v as u32),
            _ => enc.write_u64(*v),
        },
        (Type::Sequence(s), Value::Collection(items)) => {
            check_bound(items.len(), s.bound)?;
            let mark = (is_v2(enc) && !is_primitive(&s.element.ty)).then(|| enc.begin_dheader());
            enc.write_len(items.len())?;
            for item in items {
                write(item, enc, key)?;
            }
            end_dheader(enc, mark)?;
        }
        (Type::Array(a), Value::Collection(_)) => {
            let mark = (is_v2(enc) && !is_primitive(innermost(a))).then(|| enc.begin_dheader());
            write_in_array(d, enc, key)?;
            end_dheader(enc, mark)?;
        }
        (Type::Map(m), Value::Map(entries)) => {
            check_bound(entries.len(), m.bound)?;
            let primitive = is_primitive(&m.key.ty) && is_primitive(&m.element.ty);
            let mark = (is_v2(enc) && !primitive).then(|| enc.begin_dheader());
            enc.write_len(entries.len())?;
            for (k, v) in entries {
                write(k, enc, key)?;
                write(v, enc, key)?;
            }
            end_dheader(enc, mark)?;
        }
        (Type::Struct(s), Value::Struct(values)) => {
            if key {
                write_struct_key(s, values, enc)?;
            } else {
                write_struct(s, values, enc)?;
            }
        }
        (
            Type::Union(u),
            Value::Union {
                discriminator,
                member,
            },
        ) => {
            let d = discriminator_value(&discriminator.value)
                .ok_or_else(|| invalid("a discriminator that is not an integer"))?;
            if selected(u, d) != member.as_ref().map(|(i, _)| *i) {
                return Err(invalid("a union member its discriminator does not select"));
            }
            write_union(u, discriminator, member.as_ref(), enc, key)?;
        }
        _ => return Err(invalid("a value that is not of its type")),
    }
    Ok(())
}

/// Writes the elements of an array, those of the arrays inside it
/// without a DHEADER of their own.
fn write_in_array(d: &DynamicData, enc: &mut Encoder, key: bool) -> Result<()> {
    let Value::Collection(items) = &d.value else {
        return Err(invalid("a value that is not of its type"));
    };
    for item in items {
        if matches!(item.ty, Type::Array(_)) {
            write_in_array(item, enc, key)?;
        } else {
            write(item, enc, key)?;
        }
    }
    Ok(())
}

fn write_struct(s: &StructType, values: &[Option<DynamicData>], enc: &mut Encoder) -> Result<()> {
    let out = begin(enc, s.extensibility);
    for (m, v) in struct_members(s).into_iter().zip(values) {
        let must_understand = m.must_understand || m.key;
        match (s.extensibility, v) {
            (Extensibility::Mutable, Some(v)) => write_member(enc, m.id, must_understand, v)?,
            // An absent optional member of a mutable type is not written.
            (Extensibility::Mutable, None) if m.optional => {}
            (_, v) if m.optional => {
                if is_v2(enc) {
                    // Rule 20.
                    enc.write_bool(v.is_some());
                    if let Some(v) = v {
                        write(v, enc, false)?;
                    }
                } else {
                    // Rule 19: a parameter, of length 0 when absent.
                    let mark = enc.begin_parameter(m.id, must_understand)?;
                    if let Some(v) = v {
                        write(v, enc, false)?;
                    }
                    enc.end_parameter(mark)?;
                }
            }
            (_, Some(v)) => write(v, enc, false)?,
            (_, None) => return Err(invalid("a non-optional member is absent")),
        }
    }
    end(enc, out)
}

fn write_struct_key(
    s: &StructType,
    values: &[Option<DynamicData>],
    enc: &mut Encoder,
) -> Result<()> {
    let mut members: Vec<_> = struct_members(s).into_iter().zip(values).collect();
    if members.iter().any(|(m, _)| m.key) {
        members.retain(|(m, _)| m.key);
    }
    members.sort_by_key(|(m, _)| m.id);
    for (m, v) in members {
        if m.optional {
            enc.write_bool(v.is_some());
            if let Some(v) = v {
                write(v, enc, true)?;
            }
        } else {
            write(
                v.as_ref()
                    .ok_or_else(|| invalid("a non-optional member is absent"))?,
                enc,
                true,
            )?;
        }
    }
    Ok(())
}

fn write_union(
    u: &UnionType,
    discriminator: &DynamicData,
    member: Option<&(usize, Box<DynamicData>)>,
    enc: &mut Encoder,
    key: bool,
) -> Result<()> {
    if key {
        // A final union: the discriminator, then the key holder of the
        // member.
        write(discriminator, enc, false)?;
        if let Some((_, v)) = member {
            write(v, enc, true)?;
        }
        return Ok(());
    }
    let out = begin(enc, u.extensibility);
    if u.extensibility == Extensibility::Mutable {
        write_member(enc, 0, true, discriminator)?;
        if let Some((i, v)) = member {
            write_member(enc, u.members[*i].id, false, v)?;
        }
    } else {
        write(discriminator, enc, false)?;
        if let Some((_, v)) = member {
            write(v, enc, false)?;
        }
    }
    end(enc, out)
}

// ---------- reading -----------------------------------------------------

/// An aggregated value being read: its DHEADER (XCDR2, appendable and
/// mutable).
struct In {
    delimited: Option<Delimited>,
}

fn begin_read(dec: &mut Decoder<'_>, kind: Extensibility) -> Result<In> {
    let delimited = if is_v2_in(dec) && kind != Extensibility::Final {
        Some(dec.read_dheader()?)
    } else {
        None
    };
    Ok(In { delimited })
}

/// Whether an appendable value has more members: false at the end of its
/// DHEADER, when the writer had an older version of the type.
fn has_more(dec: &Decoder<'_>, input: &In) -> bool {
    input.delimited.is_none_or(|d| d.has_more(dec))
}

fn end_read(dec: &mut Decoder<'_>, input: In) -> Result<()> {
    if let Some(delimited) = input.delimited {
        dec.end_dheader(delimited)?;
    }
    Ok(())
}

/// A member of a mutable value, after its header.
enum Member {
    V2(MemberHeader),
    V1(Parameter),
}

impl Member {
    fn id(&self) -> MemberId {
        match self {
            Member::V2(h) => h.id,
            Member::V1(p) => p.id,
        }
    }

    fn must_understand(&self) -> bool {
        match self {
            Member::V2(h) => h.must_understand,
            Member::V1(p) => p.must_understand,
        }
    }
}

fn next_member(dec: &mut Decoder<'_>, input: &In) -> Result<Option<Member>> {
    match &input.delimited {
        Some(delimited) => {
            if !delimited.has_more(dec) {
                return Ok(None);
            }
            Ok(Some(Member::V2(dec.read_member_header()?)))
        }
        None => Ok(dec.read_parameter(ListEnd::Xtypes)?.map(Member::V1)),
    }
}

fn end_member(dec: &mut Decoder<'_>, member: Member) -> Result<()> {
    match member {
        Member::V2(h) => dec.end_member(h)?,
        Member::V1(p) => dec.end_parameter(p)?,
    }
    Ok(())
}

/// What a reader does with a member it does not know: skips it, or fails
/// when it must be understood (7.2.2.4.4.4.6).
fn unknown(member: &Member) -> Result<()> {
    if member.must_understand() {
        return Err(invalid("an unknown member that must be understood"));
    }
    Ok(())
}

fn read_delimited(dec: &mut Decoder<'_>, delimited: bool) -> Result<Option<Delimited>> {
    Ok(if delimited {
        Some(dec.read_dheader()?)
    } else {
        None
    })
}

fn end_delimited(dec: &mut Decoder<'_>, delimited: Option<Delimited>) -> Result<()> {
    if let Some(delimited) = delimited {
        dec.end_dheader(delimited)?;
    }
    Ok(())
}

/// Reads a value of `ty`.
fn read(ty: &Type, dec: &mut Decoder<'_>) -> Result<DynamicData> {
    let ty = resolve(ty);
    let value = match ty {
        Type::Primitive(p) => match p {
            Primitive::Boolean => Value::Boolean(dec.read_bool()?),
            Primitive::Byte => Value::Byte(dec.read_octet()?),
            Primitive::Int8 => Value::Int8(dec.read_octet()? as i8),
            Primitive::Uint8 => Value::Uint8(dec.read_octet()?),
            Primitive::Int16 => Value::Int16(dec.read_i16()?),
            Primitive::Uint16 => Value::Uint16(dec.read_u16()?),
            Primitive::Int32 => Value::Int32(dec.read_i32()?),
            Primitive::Uint32 => Value::Uint32(dec.read_u32()?),
            Primitive::Int64 => Value::Int64(dec.read_i64()?),
            Primitive::Uint64 => Value::Uint64(dec.read_u64()?),
            Primitive::Float32 => Value::Float32(dec.read_f32()?),
            Primitive::Float64 => Value::Float64(dec.read_f64()?),
            Primitive::Float128 => Value::Float128(dec.read_long_double()?),
            Primitive::Char8 => Value::Char8(char::from(dec.read_char()?)),
            Primitive::Char16 => {
                let unit = dec.read_wchar()?;
                Value::Char16(
                    char::from_u32(u32::from(unit))
                        .ok_or_else(|| invalid("a wchar that is a UTF-16 surrogate"))?,
                )
            }
        },
        Type::String { wide, bound } => {
            let s = if *wide {
                dec.read_wstring()?
            } else {
                dec.read_string()?
            };
            check_string(&s, *wide, *bound)?;
            Value::String(s)
        }
        Type::Enum(e) => {
            let v = match enum_holder_size(e.bit_bound) {
                1 => i32::from(dec.read_octet()? as i8),
                2 => i32::from(dec.read_i16()?),
                _ => dec.read_i32()?,
            };
            if !e.literals.iter().any(|l| l.value == v) {
                return Err(invalid("a value that is not a literal of its enum"));
            }
            Value::Enum(v)
        }
        Type::Bitmask(b) => Value::Bitmask(match bitmask_holder_size(b.bit_bound) {
            1 => u64::from(dec.read_octet()?),
            2 => u64::from(dec.read_u16()?),
            4 => u64::from(dec.read_u32()?),
            _ => dec.read_u64()?,
        }),
        Type::Sequence(s) => {
            let delimited = read_delimited(dec, is_v2_in(dec) && !is_primitive(&s.element.ty))?;
            let len = dec.read_len(1)?;
            check_bound(len, s.bound)?;
            let mut items = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                items.push(read(&s.element.ty, dec)?);
            }
            end_delimited(dec, delimited)?;
            Value::Collection(items)
        }
        Type::Array(a) => {
            let delimited = read_delimited(dec, is_v2_in(dec) && !is_primitive(innermost(a)))?;
            let items = read_in_array(a, dec)?;
            end_delimited(dec, delimited)?;
            Value::Collection(items)
        }
        Type::Map(m) => {
            let primitive = is_primitive(&m.key.ty) && is_primitive(&m.element.ty);
            let delimited = read_delimited(dec, is_v2_in(dec) && !primitive)?;
            let len = dec.read_len(1)?;
            check_bound(len, m.bound)?;
            let mut entries: Vec<(DynamicData, DynamicData)> = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                let k = read(&m.key.ty, dec)?;
                let v = read(&m.element.ty, dec)?;
                if entries.iter().any(|(x, _)| *x == k) {
                    return Err(invalid("a map with a key twice"));
                }
                entries.push((k, v));
            }
            end_delimited(dec, delimited)?;
            Value::Map(entries)
        }
        Type::Struct(s) => read_struct(s, dec)?,
        Type::Union(u) => read_union(u, dec)?,
        Type::Alias(_) => unreachable!("resolved"),
    };
    Ok(DynamicData {
        ty: ty.clone(),
        value,
    })
}

/// Reads the elements of an array, those of the arrays inside it without
/// a DHEADER of their own.
fn read_in_array(a: &ArrayType, dec: &mut Decoder<'_>) -> Result<Vec<DynamicData>> {
    let element = resolve(&a.element.ty);
    let len = array_len(a);
    let mut items = Vec::with_capacity(len.min(4096));
    for _ in 0..len {
        items.push(match element {
            Type::Array(inner) => DynamicData {
                ty: element.clone(),
                value: Value::Collection(read_in_array(inner, dec)?),
            },
            _ => read(element, dec)?,
        });
    }
    Ok(items)
}

/// Reads an optional member of a final or appendable type (rules 19 and
/// 20).
fn read_optional(ty: &Type, dec: &mut Decoder<'_>) -> Result<Option<DynamicData>> {
    if is_v2_in(dec) {
        return Ok(if dec.read_bool()? {
            Some(read(ty, dec)?)
        } else {
            None
        });
    }
    let param = dec.read_parameter_header()?;
    let v = if param.len == 0 {
        None
    } else {
        Some(read(ty, dec)?)
    };
    dec.end_parameter(param)?;
    Ok(v)
}

fn read_struct(s: &StructType, dec: &mut Decoder<'_>) -> Result<Value> {
    let members = struct_members(s);
    let input = begin_read(dec, s.extensibility)?;
    let mut values: Vec<Option<DynamicData>> = vec![None; members.len()];
    if s.extensibility == Extensibility::Mutable {
        while let Some(m) = next_member(dec, &input)? {
            match members.iter().position(|x| x.id == m.id()) {
                Some(i) => values[i] = Some(read(&members[i].ty, dec)?),
                None => unknown(&m)?,
            }
            end_member(dec, m)?;
        }
        for (m, v) in members.iter().zip(&mut values) {
            if v.is_none() && !m.optional {
                if m.key {
                    return Err(invalid("a key member is missing"));
                }
                *v = Some(DynamicData::new(&m.ty));
            }
        }
    } else {
        for (m, v) in members.iter().zip(&mut values) {
            *v = if !has_more(dec, &input) {
                // Not in the version of the writer: the default.
                (!m.optional).then(|| DynamicData::new(&m.ty))
            } else if m.optional {
                read_optional(&m.ty, dec)?
            } else {
                Some(read(&m.ty, dec)?)
            };
        }
    }
    end_read(dec, input)?;
    Ok(Value::Struct(values))
}

fn read_union(u: &UnionType, dec: &mut Decoder<'_>) -> Result<Value> {
    let input = begin_read(dec, u.extensibility)?;
    let (discriminator, mut got) = if u.extensibility == Extensibility::Mutable {
        let mut discriminator = None;
        let mut got = None;
        while let Some(m) = next_member(dec, &input)? {
            if discriminator.is_none() {
                discriminator = Some(read(&u.discriminator, dec)?);
            } else if let Some(i) = u.members.iter().position(|x| x.id == m.id()) {
                got = Some((i, read(&u.members[i].ty, dec)?));
            } else {
                unknown(&m)?;
            }
            end_member(dec, m)?;
        }
        (
            discriminator.ok_or_else(|| invalid("a union without its discriminator"))?,
            got,
        )
    } else {
        let discriminator = read(&u.discriminator, dec)?;
        let d = discriminator_value(&discriminator.value)
            .ok_or_else(|| invalid("a discriminator that is not an integer"))?;
        let got = match selected(u, d) {
            Some(i) if has_more(dec, &input) => Some((i, read(&u.members[i].ty, dec)?)),
            _ => None,
        };
        (discriminator, got)
    };
    end_read(dec, input)?;
    let d = discriminator_value(&discriminator.value)
        .ok_or_else(|| invalid("a discriminator that is not an integer"))?;
    let member = selected(u, d).map(|i| match got.take() {
        Some((j, v)) if j == i => (i, Box::new(v)),
        _ => (i, Box::new(DynamicData::new(&u.members[i].ty))),
    });
    Ok(Value::Union {
        discriminator: Box::new(discriminator),
        member,
    })
}

// ---------- the largest key holder ---------------------------------------

/// `offset` rounded up to a multiple of `align`, as PLAIN_CDR2 aligns (to
/// no more than 4).
fn align_up(offset: usize, align: usize) -> usize {
    let align = align.clamp(1, 4);
    offset.div_ceil(align) * align
}

fn primitive_size(p: Primitive) -> usize {
    match p {
        Primitive::Boolean
        | Primitive::Byte
        | Primitive::Int8
        | Primitive::Uint8
        | Primitive::Char8 => 1,
        Primitive::Int16 | Primitive::Uint16 | Primitive::Char16 => 2,
        Primitive::Int32 | Primitive::Uint32 | Primitive::Float32 => 4,
        Primitive::Int64 | Primitive::Uint64 | Primitive::Float64 => 8,
        Primitive::Float128 => 16,
    }
}

/// The offset after the largest key holder of `ty` written at `offset` in
/// PLAIN_CDR2, or `None` when it has no bound.
pub(crate) fn max_key_end(ty: &Type, offset: usize) -> Option<usize> {
    Some(match resolve(ty) {
        Type::Primitive(p) => {
            let n = primitive_size(*p);
            align_up(offset, n) + n
        }
        Type::String { bound: 0, .. } => return None,
        // The length, the characters and the NUL.
        Type::String { wide: false, bound } => align_up(offset, 4) + 4 + *bound as usize + 1,
        // The length and the code units, without a NUL.
        Type::String { wide: true, bound } => align_up(offset, 4) + 4 + 2 * *bound as usize,
        Type::Enum(e) => {
            let n = enum_holder_size(e.bit_bound);
            align_up(offset, n) + n
        }
        Type::Bitmask(b) => {
            let n = bitmask_holder_size(b.bit_bound);
            align_up(offset, n) + n
        }
        Type::Sequence(s) => {
            if s.bound == 0 {
                return None;
            }
            let dheader = if is_primitive(&s.element.ty) { 0 } else { 4 };
            elements_end(
                &s.element.ty,
                align_up(offset, 4) + dheader + 4,
                s.bound as usize,
            )?
        }
        Type::Array(a) => {
            let start = if is_primitive(innermost(a)) {
                offset
            } else {
                align_up(offset, 4) + 4
            };
            array_elements_end(a, start)?
        }
        Type::Map(m) => {
            if m.bound == 0 {
                return None;
            }
            let primitive = is_primitive(&m.key.ty) && is_primitive(&m.element.ty);
            let mut offset = align_up(offset, 4) + if primitive { 0 } else { 4 } + 4;
            for _ in 0..m.bound {
                offset = max_key_end(&m.key.ty, offset)?;
                offset = max_key_end(&m.element.ty, offset)?;
            }
            offset
        }
        Type::Struct(s) => {
            let mut members = struct_members(s);
            if members.iter().any(|m| m.key) {
                members.retain(|m| m.key);
            }
            members.sort_by_key(|m| m.id);
            let mut offset = offset;
            for m in members {
                if m.optional {
                    offset += 1;
                }
                offset = max_key_end(&m.ty, offset)?;
            }
            offset
        }
        Type::Union(u) => {
            let after = max_key_end(&u.discriminator, offset)?;
            let mut end = after;
            for m in &u.members {
                end = end.max(max_key_end(&m.ty, after)?);
            }
            end
        }
        Type::Alias(_) => unreachable!("resolved"),
    })
}

/// The offset after `n` elements of `ty` written at `offset`.
fn elements_end(ty: &Type, offset: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(offset);
    }
    if is_primitive(ty) {
        // After the first, the elements follow each other without padding.
        let first = max_key_end(ty, offset)?;
        let size = max_key_end(ty, 0)?;
        return first.checked_add(size.checked_mul(n - 1)?);
    }
    let mut offset = offset;
    for _ in 0..n {
        offset = max_key_end(ty, offset)?;
    }
    Some(offset)
}

/// The offset after the elements of an array, those of the arrays inside
/// it without a DHEADER of their own.
fn array_elements_end(a: &ArrayType, offset: usize) -> Option<usize> {
    match resolve(&a.element.ty) {
        Type::Array(inner) => {
            let mut offset = offset;
            for _ in 0..array_len(a) {
                offset = array_elements_end(inner, offset)?;
            }
            Some(offset)
        }
        element => elements_end(element, offset, array_len(a)),
    }
}
