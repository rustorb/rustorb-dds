//! DynamicData (7.5.2.10, 7.5.2.11): a value of a [`DynamicType`].

use rustorb_cdr::LongDouble;

use super::MEMBER_ID_INVALID;
use super::ty::{DynamicType, kind_of, resolve, struct_members};
use crate::type_object::*;
use crate::types::*;
use crate::{Error, Result};

/// Makes [`DynamicData`] (`DynamicDataFactory`, 7.5.2.10).
#[derive(Debug, Clone, Copy, Default)]
pub struct DynamicDataFactory;

impl DynamicDataFactory {
    /// `create_data`: a value of `ty` with every member at its default
    /// value (optional members absent).
    pub fn create_data(ty: &DynamicType) -> DynamicData {
        DynamicData::new(ty.as_type())
    }
}

/// A value of a [`DynamicType`] (`DynamicData`, 7.5.2.11).
///
/// Values inside it are named by member ID: the members of a struct or a
/// union, the elements of a sequence or an array (by index, arrays
/// flattened), the flags of a bitmask (by position). The value itself is
/// [`MEMBER_ID_INVALID`]. The getters and setters convert between
/// integer, floating point and character types as 7.5.2.11.9 allows, and
/// take enums as signed integers and bitmasks as unsigned ones.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicData {
    /// The type, its aliases resolved.
    pub(crate) ty: Type,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
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
    Char8(char),
    Char16(char),
    /// A string or a wstring.
    String(String),
    Enum(i32),
    Bitmask(u64),
    /// The members, in the order of [`struct_members`]; `None` for an
    /// absent optional member.
    Struct(Vec<Option<DynamicData>>),
    /// The discriminator and, when it selects one, the index of the
    /// member and its value.
    Union {
        discriminator: Box<DynamicData>,
        member: Option<(usize, Box<DynamicData>)>,
    },
    /// The elements of a sequence or (flattened) array.
    Collection(Vec<DynamicData>),
    Map(Vec<(DynamicData, DynamicData)>),
}

fn bad(what: impl Into<String>) -> Error {
    Error::BadParameter(what.into())
}

/// The default value of an enum (`@default_literal`, 7.3.1.2.1.10, else the
/// first literal).
fn enum_default(e: &EnumType) -> i32 {
    e.literals
        .iter()
        .find(|l| l.default)
        .or(e.literals.first())
        .map_or(0, |l| l.value)
}

/// The number of elements of an array.
pub(crate) fn array_len(a: &ArrayType) -> usize {
    a.dimensions.iter().map(|d| *d as usize).product()
}

/// The value of a discriminator, as an integer.
pub(crate) fn discriminator_value(v: &Value) -> Option<i64> {
    Some(match v {
        Value::Boolean(b) => i64::from(*b),
        Value::Byte(x) | Value::Uint8(x) => i64::from(*x),
        Value::Int8(x) => i64::from(*x),
        Value::Int16(x) => i64::from(*x),
        Value::Uint16(x) => i64::from(*x),
        Value::Int32(x) | Value::Enum(x) => i64::from(*x),
        Value::Uint32(x) => i64::from(*x),
        Value::Int64(x) => *x,
        Value::Uint64(x) => *x as i64,
        Value::Char8(c) | Value::Char16(c) => i64::from(u32::from(*c)),
        _ => return None,
    })
}

/// The member of `u` that the discriminator value `d` selects.
pub(crate) fn selected(u: &UnionType, d: i64) -> Option<usize> {
    u.members
        .iter()
        .position(|m| m.labels.iter().any(|l| i64::from(*l) == d))
        .or_else(|| u.members.iter().position(|m| m.default))
}

/// A discriminator value that selects the member `index` of `u`.
fn label_for(u: &UnionType, index: usize) -> Result<i64> {
    let m = &u.members[index];
    if let Some(l) = m.labels.first() {
        return Ok(i64::from(*l));
    }
    // The default member: a value no label has.
    let candidates: Box<dyn Iterator<Item = i64>> = match resolve(&u.discriminator) {
        Type::Enum(e) => Box::new(
            e.literals
                .iter()
                .map(|l| i64::from(l.value))
                .collect::<Vec<_>>()
                .into_iter(),
        ),
        Type::Primitive(Primitive::Boolean) => Box::new(0..2),
        _ => Box::new(0..),
    };
    for c in candidates.take(1 << 16) {
        if !u
            .members
            .iter()
            .any(|m| m.labels.iter().any(|l| i64::from(*l) == c))
        {
            return Ok(c);
        }
    }
    Err(bad("no discriminator value selects the default member"))
}

/// The value of a discriminator of type `ty` for the integer `d`.
fn discriminator_of(ty: &Type, d: i64) -> Result<DynamicData> {
    let mut data = DynamicData::new(ty);
    data.value = match (&data.value, resolve(ty)) {
        (Value::Enum(_), Type::Enum(e)) => {
            if !e.literals.iter().any(|l| i64::from(l.value) == d) {
                return Err(bad("not a literal of the discriminator's enum"));
            }
            Value::Enum(d as i32)
        }
        (v, _) => {
            let s = Scalar::I64(d);
            let kind = scalar_of(v)
                .ok_or_else(|| bad("not a discriminator type"))?
                .kind();
            convert(s, kind, true).ok_or_else(|| bad("discriminator value out of range"))?
        }
    };
    Ok(data)
}

impl DynamicData {
    /// A value of `ty`, at its default (7.2.2: zero, empty, the first
    /// enum literal...).
    pub(crate) fn new(ty: &Type) -> DynamicData {
        let ty = resolve(ty).clone();
        let value = match &ty {
            Type::Primitive(p) => match p {
                Primitive::Boolean => Value::Boolean(false),
                Primitive::Byte => Value::Byte(0),
                Primitive::Int8 => Value::Int8(0),
                Primitive::Uint8 => Value::Uint8(0),
                Primitive::Int16 => Value::Int16(0),
                Primitive::Uint16 => Value::Uint16(0),
                Primitive::Int32 => Value::Int32(0),
                Primitive::Uint32 => Value::Uint32(0),
                Primitive::Int64 => Value::Int64(0),
                Primitive::Uint64 => Value::Uint64(0),
                Primitive::Float32 => Value::Float32(0.0),
                Primitive::Float64 => Value::Float64(0.0),
                Primitive::Float128 => Value::Float128(LongDouble::ZERO),
                Primitive::Char8 => Value::Char8('\0'),
                Primitive::Char16 => Value::Char16('\0'),
            },
            Type::String { .. } => Value::String(String::new()),
            Type::Sequence(_) => Value::Collection(Vec::new()),
            Type::Array(a) => {
                let element = DynamicData::new(&a.element.ty);
                Value::Collection(vec![element; array_len(a)])
            }
            Type::Map(_) => Value::Map(Vec::new()),
            Type::Alias(_) => unreachable!("resolved"),
            Type::Enum(e) => Value::Enum(enum_default(e)),
            Type::Bitmask(_) => Value::Bitmask(0),
            Type::Struct(s) => Value::Struct(
                struct_members(s)
                    .iter()
                    .map(|m| (!m.optional).then(|| DynamicData::new(&m.ty)))
                    .collect(),
            ),
            Type::Union(u) => {
                let discriminator = DynamicData::new(&u.discriminator);
                let member = discriminator_value(&discriminator.value)
                    .and_then(|d| selected(u, d))
                    .map(|i| (i, Box::new(DynamicData::new(&u.members[i].ty))));
                Value::Union {
                    discriminator: Box::new(discriminator),
                    member,
                }
            }
        };
        DynamicData { ty, value }
    }

    /// `type`: the type, its aliases resolved.
    pub fn get_type(&self) -> DynamicType {
        DynamicType::new(self.ty.clone())
    }

    /// The type kind of the value.
    pub fn get_kind(&self) -> TypeKind {
        kind_of(&self.ty)
    }

    /// `equals`.
    pub fn equals(&self, other: &DynamicData) -> bool {
        self == other
    }

    /// `get_item_count` (7.5.2.11.6).
    pub fn get_item_count(&self) -> u32 {
        (match &self.value {
            Value::Collection(v) => v.len(),
            Value::Map(v) => v.len(),
            Value::Bitmask(b) => match &self.ty {
                Type::Bitmask(t) => t
                    .flags
                    .iter()
                    .filter(|f| b & (1 << f.position) != 0)
                    .count(),
                _ => 0,
            },
            Value::Struct(m) => m.iter().filter(|m| m.is_some()).count(),
            Value::Union { member, .. } => 1 + usize::from(member.is_some()),
            _ => 1,
        }) as u32
    }

    /// `get_member_id_by_name`: of a member of a struct or union, or of a
    /// flag of a bitmask; [`MEMBER_ID_INVALID`] when there is none.
    pub fn get_member_id_by_name(&self, name: &str) -> MemberId {
        match &self.ty {
            Type::Struct(s) => struct_members(s)
                .iter()
                .find(|m| m.name == name)
                .map(|m| m.id),
            Type::Union(u) => u.members.iter().find(|m| m.name == name).map(|m| m.id),
            Type::Bitmask(b) => b
                .flags
                .iter()
                .find(|f| f.name == name)
                .map(|f| MemberId::from(f.position)),
            _ => None,
        }
        .unwrap_or(MEMBER_ID_INVALID)
    }

    /// `get_member_id_at_index`: the member at `index` of a struct (among
    /// those present) or a union (0 is the discriminator, which has no
    /// ID here; 1 the selected member), the element at `index` of a
    /// collection (its index, which may be one past the end of a
    /// sequence to append).
    pub fn get_member_id_at_index(&self, index: u32) -> MemberId {
        let i = index as usize;
        match (&self.ty, &self.value) {
            (Type::Struct(s), Value::Struct(values)) => struct_members(s)
                .iter()
                .zip(values)
                .filter(|(_, v)| v.is_some())
                .nth(i)
                .map(|(m, _)| m.id),
            (Type::Union(u), Value::Union { member, .. }) => match (i, member) {
                (1, Some((m, _))) => Some(u.members[*m].id),
                _ => None,
            },
            (Type::Sequence(_), Value::Collection(v)) if i <= v.len() => Some(index),
            (_, Value::Collection(v)) if i < v.len() => Some(index),
            _ => None,
        }
        .unwrap_or(MEMBER_ID_INVALID)
    }

    /// The descriptor of member `id` (`descriptor`, 7.5.2.11.2).
    pub fn get_descriptor(&self, id: MemberId) -> Result<super::MemberDescriptor> {
        self.get_type()
            .get_member(id)
            .map(|m| m.get_descriptor().clone())
            .ok_or_else(|| bad(format!("no member {id}")))
    }

    // ---------- access by member ID -------------------------------------

    /// The value `id` names.
    fn member(&self, id: MemberId) -> Result<&DynamicData> {
        if id == MEMBER_ID_INVALID {
            return Ok(self);
        }
        match (&self.ty, &self.value) {
            (Type::Struct(s), Value::Struct(values)) => {
                let i = struct_members(s)
                    .iter()
                    .position(|m| m.id == id)
                    .ok_or_else(|| bad(format!("no member {id}")))?;
                values[i]
                    .as_ref()
                    .ok_or_else(|| bad(format!("optional member {id} is absent")))
            }
            (Type::Union(u), Value::Union { member, .. }) => match member {
                Some((i, v)) if u.members[*i].id == id => Ok(v),
                _ => Err(bad(format!("member {id} is not selected"))),
            },
            (_, Value::Collection(v)) => v
                .get(id as usize)
                .ok_or_else(|| bad(format!("no element {id}"))),
            _ => Err(bad(format!("no member {id}"))),
        }
    }

    /// The value `id` names, made when it is an absent optional member, a
    /// union member not selected (which selects it) or the element one
    /// past the end of a sequence (which appends it).
    fn member_mut(&mut self, id: MemberId) -> Result<&mut DynamicData> {
        if id == MEMBER_ID_INVALID {
            return Ok(self);
        }
        match (&self.ty, &mut self.value) {
            (Type::Struct(s), Value::Struct(values)) => {
                let members = struct_members(s);
                let i = members
                    .iter()
                    .position(|m| m.id == id)
                    .ok_or_else(|| bad(format!("no member {id}")))?;
                Ok(values[i].get_or_insert_with(|| DynamicData::new(&members[i].ty)))
            }
            (
                Type::Union(u),
                Value::Union {
                    discriminator,
                    member,
                },
            ) => {
                let i = u
                    .members
                    .iter()
                    .position(|m| m.id == id)
                    .ok_or_else(|| bad(format!("no member {id}")))?;
                if !matches!(member, Some((m, _)) if *m == i) {
                    **discriminator = discriminator_of(&u.discriminator, label_for(u, i)?)?;
                    *member = Some((i, Box::new(DynamicData::new(&u.members[i].ty))));
                }
                Ok(&mut member.as_mut().expect("selected").1)
            }
            (Type::Sequence(s), Value::Collection(v)) => {
                let i = id as usize;
                if i == v.len() {
                    if s.bound != 0 && v.len() >= s.bound as usize {
                        return Err(bad("the sequence is full"));
                    }
                    v.push(DynamicData::new(&s.element.ty));
                }
                v.get_mut(i).ok_or_else(|| bad(format!("no element {id}")))
            }
            (_, Value::Collection(v)) => v
                .get_mut(id as usize)
                .ok_or_else(|| bad(format!("no element {id}"))),
            _ => Err(bad(format!("no member {id}"))),
        }
    }

    /// `loan_value`: the value `id` names, to read or change in place
    /// (made as by the setters when it does not exist yet).
    pub fn loan_value(&mut self, id: MemberId) -> Result<&mut DynamicData> {
        self.member_mut(id)
    }

    /// The value `id` names, to read.
    pub fn get_value(&self, id: MemberId) -> Result<&DynamicData> {
        self.member(id)
    }

    /// `get_complex_value`: a copy of the value `id` names.
    pub fn get_complex_value(&self, id: MemberId) -> Result<DynamicData> {
        self.member(id).cloned()
    }

    /// `set_complex_value`: replaces the value `id` names with `value`,
    /// which must be of its type.
    pub fn set_complex_value(&mut self, id: MemberId, value: DynamicData) -> Result<()> {
        let target = self.member_mut(id)?;
        if target.ty != value.ty {
            return Err(bad("a value of another type"));
        }
        *target = value;
        Ok(())
    }

    /// Whether the optional member `id` of a struct is present.
    pub fn is_present(&self, id: MemberId) -> bool {
        self.member(id).is_ok()
    }

    /// `clear_value` (7.5.2.11.3).
    pub fn clear_value(&mut self, id: MemberId) -> Result<()> {
        if id == MEMBER_ID_INVALID {
            return self.clear_all_values();
        }
        match (&self.ty, &mut self.value) {
            (Type::Struct(s), Value::Struct(values)) => {
                let members = struct_members(s);
                let i = members
                    .iter()
                    .position(|m| m.id == id)
                    .ok_or_else(|| bad(format!("no member {id}")))?;
                values[i] = (!members[i].optional).then(|| DynamicData::new(&members[i].ty));
                Ok(())
            }
            (Type::Sequence(_), Value::Collection(v)) if (id as usize) < v.len() => {
                v.remove(id as usize);
                Ok(())
            }
            (Type::Bitmask(_), Value::Bitmask(b)) => {
                *b &= !(1u64.checked_shl(id).unwrap_or(0));
                Ok(())
            }
            _ => {
                let target = self.member_mut(id)?;
                *target = DynamicData::new(&target.ty);
                Ok(())
            }
        }
    }

    /// `clear_all_values`: every value back to its default.
    pub fn clear_all_values(&mut self) -> Result<()> {
        *self = DynamicData::new(&self.ty);
        Ok(())
    }

    /// `clear_nonkey_values`: like [`Self::clear_all_values`], but the key
    /// members of a struct keep their values.
    pub fn clear_nonkey_values(&mut self) -> Result<()> {
        if let (Type::Struct(s), Value::Struct(values)) = (&self.ty, &mut self.value) {
            for (m, v) in struct_members(s).iter().zip(values.iter_mut()) {
                if !m.key {
                    *v = (!m.optional).then(|| DynamicData::new(&m.ty));
                }
            }
            return Ok(());
        }
        self.clear_all_values()
    }

    // ---------- unions ---------------------------------------------------

    /// The discriminator of a union, as an integer (enums by their value).
    pub fn get_discriminator_value(&self) -> Result<i64> {
        match &self.value {
            Value::Union { discriminator, .. } => {
                discriminator_value(&discriminator.value).ok_or_else(|| bad("no discriminator"))
            }
            _ => Err(bad("not a union")),
        }
    }

    /// Sets the discriminator of a union. The member it selects takes its
    /// default value, unless it was already the one selected.
    pub fn set_discriminator_value(&mut self, d: i64) -> Result<()> {
        let Type::Union(u) = &self.ty else {
            return Err(bad("not a union"));
        };
        let new = discriminator_of(&u.discriminator, d)?;
        let index = selected(u, d);
        let Value::Union {
            discriminator,
            member,
        } = &mut self.value
        else {
            unreachable!("a union value");
        };
        **discriminator = new;
        if member.as_ref().map(|(i, _)| *i) != index {
            *member = index.map(|i| (i, Box::new(DynamicData::new(&u.members[i].ty))));
        }
        Ok(())
    }

    /// The ID of the member the discriminator of a union selects.
    pub fn selected_member(&self) -> Option<MemberId> {
        match (&self.ty, &self.value) {
            (
                Type::Union(u),
                Value::Union {
                    member: Some((i, _)),
                    ..
                },
            ) => Some(u.members[*i].id),
            _ => None,
        }
    }

    // ---------- maps -----------------------------------------------------

    /// The entries of a map, in order.
    pub fn map_entries(&self) -> Result<&[(DynamicData, DynamicData)]> {
        match &self.value {
            Value::Map(v) => Ok(v),
            _ => Err(bad("not a map")),
        }
    }

    /// The element of a map for `key`.
    pub fn map_get(&self, key: &DynamicData) -> Option<&DynamicData> {
        match &self.value {
            Value::Map(v) => v.iter().find(|(k, _)| k == key).map(|(_, e)| e),
            _ => None,
        }
    }

    /// Sets the element of a map for `key` (both of the map's types).
    pub fn map_insert(&mut self, key: DynamicData, element: DynamicData) -> Result<()> {
        let (Type::Map(m), Value::Map(v)) = (&self.ty, &mut self.value) else {
            return Err(bad("not a map"));
        };
        if resolve(&m.key.ty) != &key.ty || resolve(&m.element.ty) != &element.ty {
            return Err(bad("a key or an element of another type"));
        }
        if let Some(e) = v.iter_mut().find(|(k, _)| *k == key) {
            e.1 = element;
            return Ok(());
        }
        if m.bound != 0 && v.len() >= m.bound as usize {
            return Err(bad("the map is full"));
        }
        v.push((key, element));
        Ok(())
    }

    /// Removes the element of a map for `key`; whether there was one.
    pub fn map_remove(&mut self, key: &DynamicData) -> bool {
        match &mut self.value {
            Value::Map(v) => {
                let n = v.len();
                v.retain(|(k, _)| k != key);
                v.len() != n
            }
            _ => false,
        }
    }

    // ---------- strings --------------------------------------------------

    fn string(&self, id: MemberId, wide: bool) -> Result<String> {
        let target = self.member(id)?;
        match (&target.ty, &target.value) {
            (Type::String { wide: w, .. }, Value::String(s)) if *w == wide => Ok(s.clone()),
            _ => Err(bad("not of that string type")),
        }
    }

    fn set_string(&mut self, id: MemberId, value: &str, wide: bool) -> Result<()> {
        let target = self.member_mut(id)?;
        match (&target.ty, &mut target.value) {
            (Type::String { wide: w, bound }, Value::String(s)) if *w == wide => {
                let len = if wide {
                    value.encode_utf16().count()
                } else {
                    value.len()
                };
                if *bound != 0 && len > *bound as usize {
                    return Err(bad("longer than the string's bound"));
                }
                if !wide && value.contains('\0') {
                    return Err(bad("a string with a NUL"));
                }
                *s = value.to_owned();
                Ok(())
            }
            _ => Err(bad("not of that string type")),
        }
    }

    /// `get_string_value`.
    pub fn get_string_value(&self, id: MemberId) -> Result<String> {
        self.string(id, false)
    }

    /// `set_string_value`.
    pub fn set_string_value(&mut self, id: MemberId, value: &str) -> Result<()> {
        self.set_string(id, value, false)
    }

    /// `get_wstring_value`.
    pub fn get_wstring_value(&self, id: MemberId) -> Result<String> {
        self.string(id, true)
    }

    /// `set_wstring_value`.
    pub fn set_wstring_value(&mut self, id: MemberId, value: &str) -> Result<()> {
        self.set_string(id, value, true)
    }

    // ---------- primitives -----------------------------------------------

    /// The flag `position` of a bitmask, when `id` names one.
    fn flag(&self, id: MemberId) -> Option<Result<bool>> {
        match (&self.ty, &self.value) {
            (Type::Bitmask(t), Value::Bitmask(b)) if id != MEMBER_ID_INVALID => {
                Some(if id < u32::from(t.bit_bound) {
                    Ok(b & (1 << id) != 0)
                } else {
                    Err(bad("beyond the bit bound"))
                })
            }
            _ => None,
        }
    }

    fn get_scalar(&self, id: MemberId, want: TypeKind) -> Result<Scalar> {
        if want == TK_BOOLEAN {
            if let Some(flag) = self.flag(id) {
                return flag.map(Scalar::Bool);
            }
        }
        let target = self.member(id)?;
        let from = target
            .scalar()
            .ok_or_else(|| bad("not a primitive value"))?;
        let allowed = from.kind() == want
            || promotes(from.kind(), want)
            || match &target.ty {
                Type::Enum(e) => signed_bits(want).is_some_and(|b| b >= e.bit_bound),
                Type::Bitmask(m) => unsigned_bits(want).is_some_and(|b| b >= m.bit_bound),
                _ => false,
            };
        if !allowed {
            return Err(bad(format!(
                "a value of kind {:#x} read as kind {want:#x}",
                from.kind()
            )));
        }
        // Enums and bitmasks are read as integers, which always fit.
        Ok(match convert(from, want, true) {
            Some(v) => scalar_of(&v).expect("a primitive"),
            None => return Err(bad("out of range")),
        })
    }

    fn set_scalar(&mut self, id: MemberId, value: Scalar) -> Result<()> {
        if let Scalar::Bool(on) = value {
            if let Some(flag) = self.flag(id) {
                flag?;
                let Value::Bitmask(b) = &mut self.value else {
                    unreachable!("a bitmask");
                };
                if on {
                    *b |= 1 << id;
                } else {
                    *b &= !(1 << id);
                }
                return Ok(());
            }
        }
        let target = self.member_mut(id)?;
        let new = match (&target.ty, &target.value) {
            (Type::Enum(e), _) => {
                let ok = signed_bits(value.kind()).is_some_and(|b| b <= e.bit_bound.max(8))
                    || value.kind() == TK_BYTE;
                let v = value.as_i128().ok_or_else(|| bad("not an integer"))?;
                if !ok || !e.literals.iter().any(|l| i128::from(l.value) == v) {
                    return Err(bad("not a literal of the enum"));
                }
                Value::Enum(v as i32)
            }
            (Type::Bitmask(m), _) => {
                let ok = unsigned_bits(value.kind()).is_some_and(|b| b <= m.bit_bound.max(8))
                    || value.kind() == TK_BYTE;
                let v = value.as_i128().ok_or_else(|| bad("not an integer"))?;
                if !ok || v < 0 || (m.bit_bound < 64 && v >> m.bit_bound != 0) {
                    return Err(bad("beyond the bitmask's bit bound"));
                }
                Value::Bitmask(v as u64)
            }
            (_, current) => {
                let to = scalar_of(current)
                    .ok_or_else(|| bad("not a primitive value"))?
                    .kind();
                if to != value.kind() && !promotes(value.kind(), to) {
                    return Err(bad(format!(
                        "a value of kind {:#x} set as kind {to:#x}",
                        value.kind()
                    )));
                }
                convert(value, to, false).ok_or_else(|| bad("out of range"))?
            }
        };
        target.value = new;
        Ok(())
    }

    fn scalar(&self) -> Option<Scalar> {
        match (&self.ty, &self.value) {
            (Type::Enum(e), Value::Enum(v)) => Some(match e.bit_bound {
                0..=8 => Scalar::I8(*v as i8),
                9..=16 => Scalar::I16(*v as i16),
                _ => Scalar::I32(*v),
            }),
            (Type::Bitmask(m), Value::Bitmask(v)) => Some(match m.bit_bound {
                0..=8 => Scalar::U8(*v as u8),
                9..=16 => Scalar::U16(*v as u16),
                17..=32 => Scalar::U32(*v as u32),
                _ => Scalar::U64(*v),
            }),
            (_, v) => scalar_of(v),
        }
    }
}

/// A primitive value, as the getters and setters take it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scalar {
    Bool(bool),
    Byte(u8),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    F128(LongDouble),
    C8(char),
    C16(char),
}

impl Scalar {
    fn kind(self) -> TypeKind {
        match self {
            Scalar::Bool(_) => TK_BOOLEAN,
            Scalar::Byte(_) => TK_BYTE,
            Scalar::I8(_) => TK_INT8,
            Scalar::U8(_) => TK_UINT8,
            Scalar::I16(_) => TK_INT16,
            Scalar::U16(_) => TK_UINT16,
            Scalar::I32(_) => TK_INT32,
            Scalar::U32(_) => TK_UINT32,
            Scalar::I64(_) => TK_INT64,
            Scalar::U64(_) => TK_UINT64,
            Scalar::F32(_) => TK_FLOAT32,
            Scalar::F64(_) => TK_FLOAT64,
            Scalar::F128(_) => TK_FLOAT128,
            Scalar::C8(_) => TK_CHAR8,
            Scalar::C16(_) => TK_CHAR16,
        }
    }

    fn as_i128(self) -> Option<i128> {
        Some(match self {
            Scalar::Bool(b) => i128::from(b),
            Scalar::Byte(x) | Scalar::U8(x) => i128::from(x),
            Scalar::I8(x) => i128::from(x),
            Scalar::I16(x) => i128::from(x),
            Scalar::U16(x) => i128::from(x),
            Scalar::I32(x) => i128::from(x),
            Scalar::U32(x) => i128::from(x),
            Scalar::I64(x) => i128::from(x),
            Scalar::U64(x) => i128::from(x),
            Scalar::C8(c) | Scalar::C16(c) => i128::from(u32::from(c)),
            _ => return None,
        })
    }

    fn as_f64(self) -> f64 {
        match self {
            Scalar::F32(x) => f64::from(x),
            Scalar::F64(x) => x,
            Scalar::F128(x) => x.to_f64(),
            other => other.as_i128().expect("an integer") as f64,
        }
    }
}

fn scalar_of(v: &Value) -> Option<Scalar> {
    Some(match v {
        Value::Boolean(x) => Scalar::Bool(*x),
        Value::Byte(x) => Scalar::Byte(*x),
        Value::Int8(x) => Scalar::I8(*x),
        Value::Uint8(x) => Scalar::U8(*x),
        Value::Int16(x) => Scalar::I16(*x),
        Value::Uint16(x) => Scalar::U16(*x),
        Value::Int32(x) => Scalar::I32(*x),
        Value::Uint32(x) => Scalar::U32(*x),
        Value::Int64(x) => Scalar::I64(*x),
        Value::Uint64(x) => Scalar::U64(*x),
        Value::Float32(x) => Scalar::F32(*x),
        Value::Float64(x) => Scalar::F64(*x),
        Value::Float128(x) => Scalar::F128(*x),
        Value::Char8(x) => Scalar::C8(*x),
        Value::Char16(x) => Scalar::C16(*x),
        _ => return None,
    })
}

/// `s` as a value of kind `to`, or `None` when it does not fit. A byte
/// takes the bits of the target.
fn convert(s: Scalar, to: TypeKind, _reading: bool) -> Option<Value> {
    if matches!(to, TK_FLOAT32 | TK_FLOAT64 | TK_FLOAT128) {
        let f = s.as_f64();
        return Some(match (to, s) {
            (TK_FLOAT128, Scalar::F128(x)) => Value::Float128(x),
            (TK_FLOAT128, _) => Value::Float128(LongDouble::from(f)),
            (TK_FLOAT64, _) => Value::Float64(f),
            _ => Value::Float32(f as f32),
        });
    }
    let i = match s {
        // Byte converts to and from anything, bit for bit.
        Scalar::Byte(b) => i128::from(b),
        other => other.as_i128()?,
    };
    Some(match to {
        TK_BOOLEAN => match i {
            0 => Value::Boolean(false),
            1 => Value::Boolean(true),
            _ => return None,
        },
        TK_BYTE => Value::Byte(
            u8::try_from(i)
                .ok()
                .or(i8::try_from(i).ok().map(|x| x as u8))?,
        ),
        TK_INT8 => Value::Int8(i8::try_from(i).ok()?),
        TK_UINT8 => Value::Uint8(u8::try_from(i).ok()?),
        TK_INT16 => Value::Int16(i16::try_from(i).ok()?),
        TK_UINT16 => Value::Uint16(u16::try_from(i).ok()?),
        TK_INT32 => Value::Int32(i32::try_from(i).ok()?),
        TK_UINT32 => Value::Uint32(u32::try_from(i).ok()?),
        TK_INT64 => Value::Int64(i64::try_from(i).ok()?),
        TK_UINT64 => Value::Uint64(u64::try_from(i).ok()?),
        TK_CHAR8 => Value::Char8(char::from(u8::try_from(i).ok()?)),
        TK_CHAR16 => {
            let c = char::from_u32(u32::try_from(i).ok().filter(|c| *c <= 0xffff)?)?;
            Value::Char16(c)
        }
        _ => return None,
    })
}

/// Whether a value of kind `from` can be read or written as `to`
/// (7.5.2.11.9).
fn promotes(from: TypeKind, to: TypeKind) -> bool {
    const F: [TypeKind; 3] = [TK_FLOAT32, TK_FLOAT64, TK_FLOAT128];
    let list: &[TypeKind] = match from {
        TK_BYTE => return true,
        TK_INT8 => &[TK_INT16, TK_INT32, TK_INT64],
        TK_INT16 => &[TK_INT32, TK_INT64],
        TK_INT32 => &[TK_INT64, TK_FLOAT64, TK_FLOAT128],
        TK_INT64 => &[TK_FLOAT128],
        TK_UINT8 => &[
            TK_INT16, TK_INT32, TK_INT64, TK_UINT16, TK_UINT32, TK_UINT64,
        ],
        TK_UINT16 => &[TK_INT32, TK_INT64, TK_UINT32, TK_UINT64],
        TK_UINT32 => &[TK_INT64, TK_UINT64, TK_FLOAT64, TK_FLOAT128],
        TK_UINT64 => &[TK_FLOAT128],
        TK_FLOAT32 => &[TK_FLOAT64, TK_FLOAT128],
        TK_FLOAT64 => &[TK_FLOAT128],
        TK_CHAR8 => &[TK_CHAR16, TK_INT16, TK_INT32, TK_INT64],
        TK_CHAR16 => &[TK_INT32, TK_INT64],
        TK_BOOLEAN => &[
            TK_INT8, TK_INT16, TK_INT32, TK_INT64, TK_UINT8, TK_UINT16, TK_UINT32, TK_UINT64,
        ],
        _ => return false,
    };
    let to_float_too = matches!(
        from,
        TK_INT8 | TK_INT16 | TK_UINT8 | TK_UINT16 | TK_CHAR8 | TK_CHAR16 | TK_BOOLEAN
    );
    list.contains(&to) || (to_float_too && F.contains(&to))
}

fn signed_bits(kind: TypeKind) -> Option<u16> {
    match kind {
        TK_INT8 => Some(8),
        TK_INT16 => Some(16),
        TK_INT32 => Some(32),
        TK_INT64 => Some(64),
        _ => None,
    }
}

fn unsigned_bits(kind: TypeKind) -> Option<u16> {
    match kind {
        TK_UINT8 => Some(8),
        TK_UINT16 => Some(16),
        TK_UINT32 => Some(32),
        TK_UINT64 => Some(64),
        _ => None,
    }
}

macro_rules! accessors {
    ($($get:ident, $set:ident, $gets:ident, $sets:ident, $t:ty, $variant:ident, $kind:ident;)*) => {
        impl DynamicData {
            $(
                #[doc = concat!("`", stringify!($get), "`.")]
                pub fn $get(&self, id: MemberId) -> Result<$t> {
                    match self.get_scalar(id, $kind)? {
                        Scalar::$variant(v) => Ok(v),
                        _ => unreachable!("converted"),
                    }
                }

                #[doc = concat!("`", stringify!($set), "`.")]
                pub fn $set(&mut self, id: MemberId, value: $t) -> Result<()> {
                    self.set_scalar(id, Scalar::$variant(value))
                }

                #[doc = concat!("`", stringify!($gets), "`: the elements of a sequence or an array.")]
                pub fn $gets(&self, id: MemberId) -> Result<Vec<$t>> {
                    let target = self.member(id)?;
                    let Value::Collection(items) = &target.value else {
                        return Err(bad("not a sequence or an array"));
                    };
                    items.iter().map(|item| item.$get(MEMBER_ID_INVALID)).collect()
                }

                #[doc = concat!("`", stringify!($sets), "`: the elements of a sequence (all of them) or an array (as many as it has).")]
                pub fn $sets(&mut self, id: MemberId, values: &[$t]) -> Result<()> {
                    let target = self.member_mut(id)?;
                    let element = match &target.ty {
                        Type::Sequence(s) => {
                            if s.bound != 0 && values.len() > s.bound as usize {
                                return Err(bad("more than the sequence's bound"));
                            }
                            &s.element.ty
                        }
                        Type::Array(a) => {
                            if values.len() != array_len(a) {
                                return Err(bad("not as many as the array has"));
                            }
                            &a.element.ty
                        }
                        _ => return Err(bad("not a sequence or an array")),
                    };
                    let mut items = Vec::with_capacity(values.len());
                    for v in values {
                        let mut item = DynamicData::new(element);
                        item.$set(MEMBER_ID_INVALID, *v)?;
                        items.push(item);
                    }
                    target.value = Value::Collection(items);
                    Ok(())
                }
            )*
        }
    };
}

accessors! {
    get_boolean_value, set_boolean_value, get_boolean_values, set_boolean_values, bool, Bool, TK_BOOLEAN;
    get_byte_value, set_byte_value, get_byte_values, set_byte_values, u8, Byte, TK_BYTE;
    get_int8_value, set_int8_value, get_int8_values, set_int8_values, i8, I8, TK_INT8;
    get_uint8_value, set_uint8_value, get_uint8_values, set_uint8_values, u8, U8, TK_UINT8;
    get_int16_value, set_int16_value, get_int16_values, set_int16_values, i16, I16, TK_INT16;
    get_uint16_value, set_uint16_value, get_uint16_values, set_uint16_values, u16, U16, TK_UINT16;
    get_int32_value, set_int32_value, get_int32_values, set_int32_values, i32, I32, TK_INT32;
    get_uint32_value, set_uint32_value, get_uint32_values, set_uint32_values, u32, U32, TK_UINT32;
    get_int64_value, set_int64_value, get_int64_values, set_int64_values, i64, I64, TK_INT64;
    get_uint64_value, set_uint64_value, get_uint64_values, set_uint64_values, u64, U64, TK_UINT64;
    get_float32_value, set_float32_value, get_float32_values, set_float32_values, f32, F32, TK_FLOAT32;
    get_float64_value, set_float64_value, get_float64_values, set_float64_values, f64, F64, TK_FLOAT64;
    get_float128_value, set_float128_value, get_float128_values, set_float128_values, LongDouble, F128, TK_FLOAT128;
    get_char8_value, set_char8_value, get_char8_values, set_char8_values, char, C8, TK_CHAR8;
    get_char16_value, set_char16_value, get_char16_values, set_char16_values, char, C16, TK_CHAR16;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::{DynamicTypeBuilderFactory, MemberDescriptor, TypeDescriptor};
    use std::sync::Arc;

    pub(crate) fn shape_type() -> DynamicType {
        let color: Type = EnumType {
            name: "M::Color".into(),
            bit_bound: 16,
            literals: vec![
                EnumLiteral {
                    name: "RED".into(),
                    value: 3,
                    default: false,
                },
                EnumLiteral {
                    name: "BLUE".into(),
                    value: 9,
                    default: false,
                },
            ],
            ..EnumType::default()
        }
        .into();
        let u: Type = UnionType {
            name: "M::U".into(),
            discriminator: Type::Primitive(Primitive::Int16),
            members: vec![
                UnionMember::new("i", 1, Type::Primitive(Primitive::Int32), vec![1, 2]),
                UnionMember {
                    default: true,
                    ..UnionMember::new("s", 2, Type::string(), vec![])
                },
            ],
            ..UnionType::default()
        }
        .into();
        DynamicType::new(
            StructType {
                name: "M::Shape".into(),
                members: vec![
                    StructMember {
                        key: true,
                        ..StructMember::new("id", 0, Type::Primitive(Primitive::Int16))
                    },
                    StructMember::new("color", 1, color),
                    StructMember {
                        optional: true,
                        ..StructMember::new(
                            "name",
                            2,
                            Type::String {
                                wide: false,
                                bound: 5,
                            },
                        )
                    },
                    StructMember::new(
                        "points",
                        3,
                        Type::Sequence(Arc::new(SequenceType {
                            element: Element::new(Type::Primitive(Primitive::Float64)),
                            bound: 3,
                        })),
                    ),
                    StructMember::new("u", 4, u),
                    StructMember::new(
                        "perm",
                        5,
                        BitmaskType {
                            name: "M::Perm".into(),
                            bit_bound: 8,
                            flags: vec![Bitflag {
                                name: "W".into(),
                                position: 3,
                            }],
                            ..BitmaskType::default()
                        }
                        .into(),
                    ),
                ],
                ..StructType::default()
            }
            .into(),
        )
    }

    #[test]
    fn defaults() {
        let d = DynamicDataFactory::create_data(&shape_type());
        assert_eq!(d.get_int16_value(0), Ok(0));
        // The first literal.
        assert_eq!(d.get_int32_value(1), Ok(3));
        // Absent optional member.
        assert!(!d.is_present(2));
        assert_eq!(d.get_item_count(), 5);
        // The union's default discriminator (0) selects the default case.
        let u = d.get_value(4).unwrap();
        assert_eq!(u.get_discriminator_value(), Ok(0));
        assert_eq!(u.selected_member(), Some(2));
    }

    #[test]
    fn getters_and_setters_with_promotions() {
        let mut d = DynamicDataFactory::create_data(&shape_type());
        d.set_int16_value(0, 7).unwrap();
        // Int16 reads as Int32, Int64, Float32, Float64...
        assert_eq!(d.get_int32_value(0), Ok(7));
        assert_eq!(d.get_float64_value(0), Ok(7.0));
        // ...but not as Int8, and an Int32 cannot be set into it.
        assert!(d.get_int8_value(0).is_err());
        assert!(d.set_int32_value(0, 7).is_err());
        // Int8 and UInt8 promote to Int16.
        d.set_int8_value(0, -2).unwrap();
        assert_eq!(d.get_int16_value(0), Ok(-2));
        // Enums: as signed integers of at least their bit bound, and only
        // their literals.
        d.set_int16_value(1, 9).unwrap();
        assert_eq!(d.get_int32_value(1), Ok(9));
        assert!(d.get_int8_value(1).is_err());
        assert!(d.set_int16_value(1, 4).is_err());
        // Strings, bounded.
        d.set_string_value(2, "hello").unwrap();
        assert_eq!(d.get_string_value(2).as_deref(), Ok("hello"));
        assert!(d.set_string_value(2, "toolong").is_err());
        assert!(d.get_wstring_value(2).is_err());
        d.clear_value(2).unwrap();
        assert!(!d.is_present(2));
        // Sequences, bounded; append one past the end.
        d.set_float64_values(3, &[1.0, 2.0]).unwrap();
        let points = d.loan_value(3).unwrap();
        assert_eq!(points.get_member_id_at_index(2), 2);
        points.set_float64_value(2, 3.0).unwrap();
        assert!(points.set_float64_value(3, 4.0).is_err());
        assert_eq!(d.get_float64_values(3), Ok(vec![1.0, 2.0, 3.0]));
        assert!(d.set_float64_values(3, &[0.0; 4]).is_err());
        // Unions: setting a member selects it.
        let u = d.loan_value(4).unwrap();
        u.set_int32_value(1, 42).unwrap();
        assert_eq!(u.get_discriminator_value(), Ok(1));
        assert!(u.get_string_value(2).is_err());
        u.set_discriminator_value(2).unwrap();
        // Still the same member: its value stays.
        assert_eq!(u.get_int32_value(1), Ok(42));
        u.set_discriminator_value(5).unwrap();
        assert_eq!(u.selected_member(), Some(2));
        // Bitmask flags, by position.
        let id = d.get_value(5).unwrap().get_member_id_by_name("W");
        assert_eq!(id, 3);
        let perm = d.loan_value(5).unwrap();
        perm.set_boolean_value(3, true).unwrap();
        assert_eq!(perm.get_uint8_value(MEMBER_ID_INVALID), Ok(8));
        assert_eq!(perm.get_item_count(), 1);
        assert!(perm.set_uint8_value(MEMBER_ID_INVALID, 0).is_ok());
        // Clearing keeps the key.
        d.clear_nonkey_values().unwrap();
        assert_eq!(d.get_int16_value(0), Ok(-2));
        assert_eq!(d.get_float64_values(3), Ok(vec![]));
        // Equality, cloning.
        let c = d.clone();
        assert!(c.equals(&d));
        d.clear_all_values().unwrap();
        assert!(!c.equals(&d));
    }

    #[test]
    fn complex_values_and_maps() {
        let mut d = DynamicDataFactory::create_data(&shape_type());
        let mut u = d.get_complex_value(4).unwrap();
        u.set_string_value(2, "x").unwrap();
        d.set_complex_value(4, u.clone()).unwrap();
        assert_eq!(d.get_value(4), Ok(&u));
        assert!(d.set_complex_value(0, u).is_err());

        let string = DynamicTypeBuilderFactory::create_string_type(0)
            .unwrap()
            .build()
            .unwrap();
        let int = DynamicTypeBuilderFactory::get_primitive_type(TK_INT32).unwrap();
        let map = DynamicTypeBuilderFactory::create_map_type(&string, &int, 1)
            .unwrap()
            .build()
            .unwrap();
        let mut m = DynamicDataFactory::create_data(&map);
        let mut k = DynamicDataFactory::create_data(&string);
        k.set_string_value(MEMBER_ID_INVALID, "a").unwrap();
        let mut v = DynamicDataFactory::create_data(&int);
        v.set_int32_value(MEMBER_ID_INVALID, 1).unwrap();
        m.map_insert(k.clone(), v.clone()).unwrap();
        assert_eq!(m.map_get(&k), Some(&v));
        // Bound 1.
        let mut k2 = k.clone();
        k2.set_string_value(MEMBER_ID_INVALID, "b").unwrap();
        assert!(m.map_insert(k2, v.clone()).is_err());
        assert!(m.map_insert(v.clone(), v).is_err());
        assert!(m.map_remove(&k));
        assert_eq!(m.get_item_count(), 0);

        // A struct built at run time, with an array.
        let arr = DynamicTypeBuilderFactory::create_array_type(&int, &[2, 2])
            .unwrap()
            .build()
            .unwrap();
        let mut b =
            DynamicTypeBuilderFactory::create_type(TypeDescriptor::new(TK_STRUCTURE, "M::A"))
                .unwrap();
        b.add_member(MemberDescriptor {
            name: "a".into(),
            id: 0,
            r#type: arr,
            ..Default::default()
        })
        .unwrap();
        let mut a = DynamicDataFactory::create_data(&b.build().unwrap());
        assert_eq!(a.get_int32_values(0), Ok(vec![0; 4]));
        a.loan_value(0).unwrap().set_int32_value(3, 5).unwrap();
        assert!(a.loan_value(0).unwrap().set_int32_value(4, 5).is_err());
        assert_eq!(a.get_int32_values(0), Ok(vec![0, 0, 0, 5]));
        assert_eq!(a.get_member_id_by_name("a"), 0);
        assert_eq!(a.get_member_id_by_name("b"), MEMBER_ID_INVALID);
    }
}
