//! CORBA TypeCodes and anys (rustorb) as DDS-XTypes types and
//! DynamicData (rustorb-dds), and back.
//!
//! - [`dynamic_type`]: a [`TypeCode`] as a [`DynamicType`], whose
//!   TypeObject ([`TypeObjects::new`](rustorb_xtypes::types::TypeObjects::new))
//!   is that of the type rustorb-idl and other IDL compilers describe for
//!   the same IDL, given the same [`Options`].
//! - [`type_code`]: a [`DynamicType`] as a [`TypeCode`]; with
//!   [`DynamicTypeBuilderFactory::create_type_w_type_object`](rustorb_xtypes::dynamic::DynamicTypeBuilderFactory::create_type_w_type_object),
//!   a TypeObject as a TypeCode.
//! - [`dynamic_data`] and [`any`]: an [`Any`] as a [`DynamicData`] of a
//!   type made by [`dynamic_type`], and back.
//!
//! A TypeCode says less than a TypeObject: it has no extensibility, no
//! `@key`, `@optional` or member IDs. The rules (docs/corba.md):
//!
//! - structs and unions take the extensibility of [`Options`]
//!   (appendable by default, the default of IDL 4), enums are final
//! - the key members of a struct are those [`Options::key`] names
//! - member IDs are sequential from 0, in declaration order
//!   (`@autoid(SEQUENTIAL)`), the members of a union too
//! - the qualified name comes from the repository ID (`IDL:A/B/C:1.0` is
//!   `A::B::C`; leading parts with a dot, from `#pragma prefix`, are left
//!   out), and back (`A::B::C` is `IDL:A/B/C:1.0`)
//! - an enumerator's value is its position
//! - nested arrays (`long m[2][3]`) are one array of several dimensions
//!
//! TypeCodes of kinds DDS-XTypes has no type for (`any`, object
//! references, valuetypes, `fixed`, exceptions, `TypeCode`...) and
//! recursive types are [`Error::Unsupported`], and so are, the other way,
//! `map`, `bitmask`, `int8` and optional members.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

pub use rustorb_core;
pub use rustorb_xtypes;

use rustorb_core::typecode::{TCKind, TypeCode, TypeCodeError};
use rustorb_core::{Any, Value};
use rustorb_xtypes::dynamic::{DynamicData, DynamicDataFactory, DynamicType, MEMBER_ID_INVALID};
use rustorb_xtypes::type_object::MemberId;
use rustorb_xtypes::types::{
    AliasType, ArrayType, Element, EnumLiteral, EnumType, Extensibility, Primitive, SequenceType,
    StructMember, StructType, Type, UnionMember, UnionType,
};

/// Errors of the conversions.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A type or value the other side has no form for.
    Unsupported(String),
    /// A value that is not of the type it was given with, or options that
    /// name what the type does not have.
    Mismatch(String),
    /// A TypeCode operation failed.
    TypeCode(TypeCodeError),
    /// A DynamicType or DynamicData operation failed.
    Xtypes(rustorb_xtypes::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(what) => write!(f, "not supported: {what}"),
            Error::Mismatch(what) => write!(f, "mismatch: {what}"),
            Error::TypeCode(e) => write!(f, "TypeCode: {e}"),
            Error::Xtypes(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::TypeCode(e) => Some(e),
            Error::Xtypes(e) => Some(e),
            _ => None,
        }
    }
}

impl From<TypeCodeError> for Error {
    fn from(e: TypeCodeError) -> Error {
        Error::TypeCode(e)
    }
}

impl From<rustorb_xtypes::Error> for Error {
    fn from(e: rustorb_xtypes::Error) -> Error {
        Error::Xtypes(e)
    }
}

/// The result of the conversions.
pub type Result<T> = std::result::Result<T, Error>;

fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::Unsupported(what.into()))
}

fn mismatch<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::Mismatch(what.into()))
}

/// What [`dynamic_type`] cannot read from a TypeCode: extensibility and
/// keys, given by the qualified names of the types (`Keyed::Shape`).
///
/// ```
/// use rustorb_dds_corba::Options;
/// use rustorb_xtypes::types::Extensibility;
///
/// let options = Options::new()
///     .default_extensibility(Extensibility::Final)
///     .key("Keyed::Shape", "id");
/// ```
#[derive(Debug, Clone)]
pub struct Options {
    default_extensibility: Extensibility,
    extensibility: HashMap<String, Extensibility>,
    keys: HashMap<String, Vec<String>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            default_extensibility: Extensibility::Appendable,
            extensibility: HashMap::new(),
            keys: HashMap::new(),
        }
    }
}

impl Options {
    /// Appendable structs and unions, without keys.
    pub fn new() -> Options {
        Options::default()
    }

    /// The extensibility of the structs and unions [`Self::extensibility`]
    /// does not name (appendable when not set).
    pub fn default_extensibility(mut self, extensibility: Extensibility) -> Options {
        self.default_extensibility = extensibility;
        self
    }

    /// The extensibility of the struct or union `type_name`.
    pub fn extensibility(mut self, type_name: &str, extensibility: Extensibility) -> Options {
        self.extensibility.insert(type_name.into(), extensibility);
        self
    }

    /// Makes the member `member` of the struct `type_name` a key member
    /// (`@key`).
    pub fn key(mut self, type_name: &str, member: &str) -> Options {
        self.keys
            .entry(type_name.into())
            .or_default()
            .push(member.into());
        self
    }

    fn extensibility_of(&self, name: &str) -> Extensibility {
        self.extensibility
            .get(name)
            .copied()
            .unwrap_or(self.default_extensibility)
    }
}

// ---------- names --------------------------------------------------------

/// The qualified name of the named type `tc`: from its repository ID when
/// that is an `IDL:` one ending in the type's name, else its name.
fn qualified_name(tc: &TypeCode) -> Result<String> {
    let id = tc.id()?;
    let name = tc.name()?;
    if let Some(rest) = id.strip_prefix("IDL:") {
        let body = rest.rsplit_once(':').map_or(rest, |(body, _version)| body);
        let mut parts: Vec<&str> = body.split('/').collect();
        while parts.len() > 1 && parts[0].contains('.') {
            parts.remove(0);
        }
        if parts.last() == Some(&name) && parts.iter().all(|p| !p.is_empty()) {
            return Ok(parts.join("::"));
        }
    }
    if name.is_empty() {
        return unsupported(format!("a type with no name ({id})"));
    }
    Ok(name.to_owned())
}

/// The repository ID and the simple name of the type `qualified`.
fn id_and_name(qualified: &str) -> (String, String) {
    let simple = qualified.rsplit("::").next().unwrap_or(qualified);
    (
        format!("IDL:{}:1.0", qualified.replace("::", "/")),
        simple.to_owned(),
    )
}

// ---------- TypeCode -> Type ---------------------------------------------

/// The DDS-XTypes type of `tc`, by the rules of the [crate]
/// documentation and `options`.
pub fn dynamic_type(tc: &TypeCode, options: &Options) -> Result<DynamicType> {
    let ty = ToType {
        options,
        done: HashMap::new(),
        busy: HashSet::new(),
    }
    .convert(tc)?;
    Ok(DynamicType::new(ty))
}

struct ToType<'a> {
    options: &'a Options,
    /// Named types already made, by repository ID.
    done: HashMap<String, Type>,
    /// Named types being made, against recursion.
    busy: HashSet<String>,
}

impl ToType<'_> {
    fn convert(&mut self, tc: &TypeCode) -> Result<Type> {
        if tc.is_recursive_placeholder() {
            return unsupported("a recursive type");
        }
        let kind = tc.kind();
        let primitive = |p| Ok(Type::Primitive(p));
        match kind {
            TCKind::Boolean => return primitive(Primitive::Boolean),
            TCKind::Octet => return primitive(Primitive::Byte),
            TCKind::Short => return primitive(Primitive::Int16),
            TCKind::UShort => return primitive(Primitive::Uint16),
            TCKind::Long => return primitive(Primitive::Int32),
            TCKind::ULong => return primitive(Primitive::Uint32),
            TCKind::LongLong => return primitive(Primitive::Int64),
            TCKind::ULongLong => return primitive(Primitive::Uint64),
            TCKind::Float => return primitive(Primitive::Float32),
            TCKind::Double => return primitive(Primitive::Float64),
            TCKind::LongDouble => return primitive(Primitive::Float128),
            TCKind::Char => return primitive(Primitive::Char8),
            TCKind::WChar => return primitive(Primitive::Char16),
            TCKind::String | TCKind::WString => {
                return Ok(Type::String {
                    wide: kind == TCKind::WString,
                    bound: tc.length()?,
                });
            }
            TCKind::Sequence => {
                return Ok(Type::Sequence(Arc::new(SequenceType {
                    element: Element::new(self.convert(&tc.content_type()?)?),
                    bound: tc.length()?,
                })));
            }
            TCKind::Array => {
                let mut dimensions = vec![tc.length()?];
                let mut element = tc.content_type()?;
                while element.kind() == TCKind::Array {
                    dimensions.push(element.length()?);
                    element = element.content_type()?;
                }
                return Ok(Type::Array(Arc::new(ArrayType {
                    element: Element::new(self.convert(&element)?),
                    dimensions,
                })));
            }
            TCKind::Alias | TCKind::Enum | TCKind::Struct | TCKind::Union => {}
            _ => return unsupported(format!("TypeCodes of kind {}", kind.name())),
        }

        let id = tc.id()?.to_owned();
        if let Some(ty) = self.done.get(&id) {
            return Ok(ty.clone());
        }
        if !self.busy.insert(id.clone()) {
            return unsupported(format!("the recursive type {id}"));
        }
        let name = qualified_name(tc)?;
        let ty = match kind {
            TCKind::Alias => Type::from(AliasType {
                related: self.convert(&tc.content_type()?)?,
                name,
            }),
            TCKind::Enum => Type::from(EnumType {
                literals: (0..tc.member_count()?)
                    .map(|i| {
                        Ok(EnumLiteral {
                            name: tc.member_name(i)?.to_owned(),
                            value: i as i32,
                            default: false,
                        })
                    })
                    .collect::<Result<_>>()?,
                name,
                ..EnumType::default()
            }),
            TCKind::Struct => self.struct_type(tc, name)?,
            TCKind::Union => self.union_type(tc, name)?,
            _ => unreachable!("named kinds"),
        };
        self.busy.remove(&id);
        self.done.insert(id, ty.clone());
        Ok(ty)
    }

    fn struct_type(&mut self, tc: &TypeCode, name: String) -> Result<Type> {
        let keys = self.options.keys.get(&name).map_or(&[][..], Vec::as_slice);
        let mut members = Vec::new();
        for i in 0..tc.member_count()? {
            let member_name = tc.member_name(i)?;
            let key = keys.iter().any(|m| m == member_name);
            members.push(StructMember {
                key,
                must_understand: key,
                ..StructMember::new(member_name, i, self.convert(&tc.member_type(i)?)?)
            });
        }
        if let Some(k) = keys
            .iter()
            .find(|k| !members.iter().any(|m: &StructMember| &m.name == *k))
        {
            return mismatch(format!("no member {k} in {name} to make a key"));
        }
        Ok(Type::from(StructType {
            extensibility: self.options.extensibility_of(&name),
            members,
            name,
            ..StructType::default()
        }))
    }

    fn union_type(&mut self, tc: &TypeCode, name: String) -> Result<Type> {
        let discriminator = self.convert(&tc.discriminator_type()?)?;
        let mut members: Vec<UnionMember> = Vec::new();
        for i in 0..tc.member_count()? {
            let member_name = tc.member_name(i)?;
            // A member with several labels is one entry for each, one after
            // the other.
            let same = members.last().is_some_and(|m| m.name == member_name);
            if !same {
                let ty = self.convert(&tc.member_type(i)?)?;
                let id = members.len() as MemberId;
                members.push(UnionMember::new(member_name, id, ty, Vec::new()));
            }
            let member = members.last_mut().expect("pushed");
            match tc.member_label_value(i)? {
                None => member.default = true,
                Some(label) => member.labels.push(i32::try_from(label).or_else(|_| {
                    unsupported(format!(
                        "the union label {label}, out of the range of a long"
                    ))
                })?),
            }
        }
        Ok(Type::from(UnionType {
            extensibility: self.options.extensibility_of(&name),
            discriminator,
            members,
            name,
            ..UnionType::default()
        }))
    }
}

// ---------- Type -> TypeCode ---------------------------------------------

/// The TypeCode of `ty`, by the rules of the [crate] documentation:
/// extensibility, keys and member IDs are left out.
pub fn type_code(ty: &DynamicType) -> Result<TypeCode> {
    ToTypeCode {
        done: HashMap::new(),
    }
    .convert(ty.as_type())
}

struct ToTypeCode {
    /// Named types already made, by qualified name.
    done: HashMap<String, TypeCode>,
}

fn resolve(mut ty: &Type) -> &Type {
    while let Type::Alias(a) = ty {
        ty = &a.related;
    }
    ty
}

/// The members of a struct, those of its base types first.
fn struct_members(s: &StructType) -> Vec<&StructMember> {
    let mut out = match s.base.as_ref().map(resolve) {
        Some(Type::Struct(base)) => struct_members(base),
        _ => Vec::new(),
    };
    out.extend(&s.members);
    out
}

impl ToTypeCode {
    fn convert(&mut self, ty: &Type) -> Result<TypeCode> {
        let simple = |kind| Ok(TypeCode::simple(kind)?);
        let name = match ty {
            Type::Primitive(p) => {
                return simple(match p {
                    Primitive::Boolean => TCKind::Boolean,
                    Primitive::Byte | Primitive::Uint8 => TCKind::Octet,
                    Primitive::Int8 => return unsupported("int8"),
                    Primitive::Int16 => TCKind::Short,
                    Primitive::Uint16 => TCKind::UShort,
                    Primitive::Int32 => TCKind::Long,
                    Primitive::Uint32 => TCKind::ULong,
                    Primitive::Int64 => TCKind::LongLong,
                    Primitive::Uint64 => TCKind::ULongLong,
                    Primitive::Float32 => TCKind::Float,
                    Primitive::Float64 => TCKind::Double,
                    Primitive::Float128 => TCKind::LongDouble,
                    Primitive::Char8 => TCKind::Char,
                    Primitive::Char16 => TCKind::WChar,
                });
            }
            Type::String { wide: false, bound } => return Ok(TypeCode::string(*bound)),
            Type::String { wide: true, bound } => return Ok(TypeCode::wstring(*bound)),
            Type::Sequence(s) => {
                let element = self.convert(&s.element.ty)?;
                return Ok(TypeCode::sequence(&element, s.bound)?);
            }
            Type::Array(a) => {
                let mut tc = self.convert(&a.element.ty)?;
                for d in a.dimensions.iter().rev() {
                    tc = TypeCode::array(&tc, *d)?;
                }
                return Ok(tc);
            }
            Type::Map(_) => return unsupported("map"),
            Type::Bitmask(b) => return unsupported(format!("the bitmask {}", b.name)),
            Type::Alias(a) => &a.name,
            Type::Enum(e) => &e.name,
            Type::Struct(s) => &s.name,
            Type::Union(u) => &u.name,
        };
        if let Some(tc) = self.done.get(name) {
            return Ok(tc.clone());
        }
        let (id, simple_name) = id_and_name(name);
        let tc = match ty {
            Type::Alias(a) => TypeCode::alias(&id, &simple_name, &self.convert(&a.related)?)?,
            Type::Enum(e) => {
                let names: Vec<&str> = e.literals.iter().map(|l| l.name.as_str()).collect();
                TypeCode::enum_tc(&id, &simple_name, &names)?
            }
            Type::Struct(s) => {
                let mut members = Vec::new();
                for m in struct_members(s) {
                    if m.optional {
                        return unsupported(format!("the optional member {}::{}", s.name, m.name));
                    }
                    members.push((m.name.as_str(), self.convert(&m.ty)?));
                }
                TypeCode::struct_tc(&id, &simple_name, &members)?
            }
            Type::Union(u) => {
                let discriminator = self.convert(&u.discriminator)?;
                let literals = match resolve(&u.discriminator) {
                    Type::Enum(e) => Some(&e.literals),
                    _ => None,
                };
                let mut members = Vec::new();
                for m in &u.members {
                    let tc = self.convert(&m.ty)?;
                    for label in &m.labels {
                        // Enum labels are values in a UnionType, positions
                        // in a TypeCode.
                        let label = match literals {
                            Some(l) => {
                                l.iter().position(|l| l.value == *label).ok_or_else(|| {
                                    Error::Mismatch(format!("the label {label} of {}", u.name))
                                })? as i128
                            }
                            None => i128::from(*label),
                        };
                        members.push((Some(label), m.name.as_str(), tc.clone()));
                    }
                    if m.default {
                        members.push((None, m.name.as_str(), tc));
                    }
                }
                TypeCode::union_tc(&id, &simple_name, &discriminator, &members)?
            }
            _ => unreachable!("named types"),
        };
        self.done.insert(name.clone(), tc.clone());
        Ok(tc)
    }
}

// ---------- Any -> DynamicData -------------------------------------------

/// The value of `any` as a value of `ty`, a type [`dynamic_type`] made
/// from the TypeCode of `any` (or one with the same names and shape).
/// Members of `ty` the TypeCode does not have keep their default values.
pub fn dynamic_data(any: &Any, ty: &DynamicType) -> Result<DynamicData> {
    to_data(any.typecode(), any.value(), ty.as_type())
}

const SELF: MemberId = MEMBER_ID_INVALID;

/// The elements of a list value: `Octets` and `Chars` as lists too.
fn elements(v: &Value) -> Result<Vec<Value>> {
    Ok(match v {
        Value::List(items) => items.clone(),
        Value::Octets(o) => o.as_slice().iter().map(|b| Value::Octet(*b)).collect(),
        Value::Chars(s) => s.chars().map(Value::Char).collect(),
        _ => return mismatch("not a sequence or an array value"),
    })
}

fn to_data(tc: &TypeCode, v: &Value, ty: &Type) -> Result<DynamicData> {
    let tc = tc.unaliased();
    let ty = resolve(ty);
    let mut d = DynamicDataFactory::create_data(&DynamicType::new(ty.clone()));
    match (v, ty) {
        (Value::Bool(x), _) => d.set_boolean_value(SELF, *x)?,
        (Value::Octet(x), Type::Primitive(Primitive::Uint8)) => d.set_uint8_value(SELF, *x)?,
        (Value::Octet(x), _) => d.set_byte_value(SELF, *x)?,
        (Value::Short(x), _) => d.set_int16_value(SELF, *x)?,
        (Value::UShort(x), _) => d.set_uint16_value(SELF, *x)?,
        (Value::Long(x), _) => d.set_int32_value(SELF, *x)?,
        (Value::ULong(x), _) => d.set_uint32_value(SELF, *x)?,
        (Value::LongLong(x), _) => d.set_int64_value(SELF, *x)?,
        (Value::ULongLong(x), _) => d.set_uint64_value(SELF, *x)?,
        (Value::Float(x), _) => d.set_float32_value(SELF, *x)?,
        (Value::Double(x), _) => d.set_float64_value(SELF, *x)?,
        (Value::LongDouble(x), _) => {
            d.set_float128_value(SELF, rustorb_cdr::LongDouble::from_bits(x.to_bits()))?
        }
        (Value::Char(c), _) => d.set_char8_value(SELF, *c)?,
        (Value::WChar(c), _) => {
            let c = char::from_u32(u32::from(*c))
                .ok_or_else(|| Error::Unsupported(format!("the wchar {c:#06x}, a surrogate")))?;
            d.set_char16_value(SELF, c)?
        }
        (Value::String(s), Type::String { wide: false, .. }) => d.set_string_value(SELF, s)?,
        (Value::String(s), Type::String { wide: true, .. }) => d.set_wstring_value(SELF, s)?,
        (Value::Enum(position), Type::Enum(e)) => {
            let literal = e.literals.get(*position as usize).ok_or_else(|| {
                Error::Mismatch(format!("no enumerator {position} in {}", e.name))
            })?;
            d.set_int32_value(SELF, literal.value)?
        }
        (Value::List(items), Type::Struct(s)) => {
            if items.len() != tc.member_count()? as usize {
                return mismatch(format!(
                    "a value with the wrong number of members for {}",
                    s.name
                ));
            }
            let members = struct_members(s);
            for (i, item) in items.iter().enumerate() {
                let name = tc.member_name(i as u32)?;
                let m = members
                    .iter()
                    .find(|m| m.name == name)
                    .ok_or_else(|| Error::Mismatch(format!("no member {name} in {}", s.name)))?;
                let value = to_data(&tc.member_type(i as u32)?, item, &m.ty)?;
                d.set_complex_value(m.id, value)?;
            }
        }
        (Value::Union { disc, member }, Type::Union(u)) => {
            let label = disc
                .label_key()
                .ok_or_else(|| Error::Mismatch("not a discriminator value".into()))?;
            let discriminator = match (resolve(&u.discriminator), disc.as_ref()) {
                (Type::Enum(e), Value::Enum(p)) => e
                    .literals
                    .get(*p as usize)
                    .map(|l| i64::from(l.value))
                    .ok_or_else(|| Error::Mismatch(format!("no enumerator {p}")))?,
                _ => i64::try_from(label)
                    .map_err(|_| Error::Unsupported(format!("the discriminator {label}")))?,
            };
            if let Some((index, value)) = member {
                let name = tc.member_name(*index)?;
                let m =
                    u.members.iter().find(|m| m.name == name).ok_or_else(|| {
                        Error::Mismatch(format!("no member {name} in {}", u.name))
                    })?;
                let value = to_data(&tc.member_type(*index)?, value, &m.ty)?;
                d.set_complex_value(m.id, value)?;
                d.set_discriminator_value(discriminator)?;
                if d.selected_member() != Some(m.id) {
                    return mismatch(format!(
                        "the discriminator {discriminator} does not select {}::{name}",
                        u.name
                    ));
                }
            } else {
                d.set_discriminator_value(discriminator)?;
                if d.selected_member().is_some() {
                    return mismatch(format!(
                        "the discriminator {discriminator} selects a member of {}",
                        u.name
                    ));
                }
            }
        }
        (_, Type::Sequence(s)) => {
            let element = tc.content_type()?;
            for (i, item) in elements(v)?.iter().enumerate() {
                let value = to_data(&element, item, &s.element.ty)?;
                d.set_complex_value(i as MemberId, value)?;
            }
        }
        (_, Type::Array(a)) => {
            // Flattened: the elements of the innermost arrays, in order.
            let mut items = vec![v.clone()];
            let mut element = tc;
            for _ in &a.dimensions {
                if element.kind() != TCKind::Array {
                    return mismatch("an array of fewer dimensions");
                }
                let mut next = Vec::new();
                for item in &items {
                    let e = elements(item)?;
                    if e.len() != element.length()? as usize {
                        return mismatch("an array value of the wrong length");
                    }
                    next.extend(e);
                }
                items = next;
                element = element.content_type()?.unaliased();
            }
            for (i, item) in items.iter().enumerate() {
                let value = to_data(&element, item, &a.element.ty)?;
                d.set_complex_value(i as MemberId, value)?;
            }
        }
        _ => {
            return mismatch(format!(
                "a value of TypeCode kind {} for a DDS-XTypes {:?}",
                tc.kind().name(),
                DynamicType::new(ty.clone()).get_kind()
            ));
        }
    }
    Ok(d)
}

// ---------- DynamicData -> Any -------------------------------------------

/// `data` as an any of type `tc`, the TypeCode [`type_code`] makes from
/// the type of `data` (or one with the same names and shape).
pub fn any(data: &DynamicData, tc: &TypeCode) -> Result<Any> {
    Ok(Any::new(tc.clone(), to_value(data, tc)?))
}

/// `items` as a list value, as `Octets` or `Chars` for elements of
/// `octet` and `char` (as rustorb reads them).
fn list(items: Vec<Value>, element: &TypeCode) -> Value {
    match element.unaliased().kind() {
        TCKind::Octet => Value::Octets(
            items
                .iter()
                .map(|v| match v {
                    Value::Octet(b) => *b,
                    _ => unreachable!("octets"),
                })
                .collect::<Vec<u8>>()
                .into(),
        ),
        TCKind::Char => Value::Chars(
            items
                .iter()
                .map(|v| match v {
                    Value::Char(c) => *c,
                    _ => unreachable!("chars"),
                })
                .collect(),
        ),
        _ => Value::List(items),
    }
}

fn to_value(d: &DynamicData, tc: &TypeCode) -> Result<Value> {
    let tc = tc.unaliased();
    let ty = d.get_type();
    let ty = ty.as_type();
    Ok(match tc.kind() {
        TCKind::Boolean => Value::Bool(d.get_boolean_value(SELF)?),
        TCKind::Octet => Value::Octet(match ty {
            Type::Primitive(Primitive::Uint8) => d.get_uint8_value(SELF)?,
            _ => d.get_byte_value(SELF)?,
        }),
        TCKind::Short => Value::Short(d.get_int16_value(SELF)?),
        TCKind::UShort => Value::UShort(d.get_uint16_value(SELF)?),
        TCKind::Long => Value::Long(d.get_int32_value(SELF)?),
        TCKind::ULong => Value::ULong(d.get_uint32_value(SELF)?),
        TCKind::LongLong => Value::LongLong(d.get_int64_value(SELF)?),
        TCKind::ULongLong => Value::ULongLong(d.get_uint64_value(SELF)?),
        TCKind::Float => Value::Float(d.get_float32_value(SELF)?),
        TCKind::Double => Value::Double(d.get_float64_value(SELF)?),
        TCKind::LongDouble => Value::LongDouble(rustorb_core::LongDouble::from_bits(
            d.get_float128_value(SELF)?.to_bits(),
        )),
        TCKind::Char => Value::Char(d.get_char8_value(SELF)?),
        TCKind::WChar => {
            let c = d.get_char16_value(SELF)?;
            let mut units = [0u16; 2];
            match c.encode_utf16(&mut units) {
                [unit] => Value::WChar(*unit),
                _ => return unsupported(format!("the wchar {c:?}, outside the BMP")),
            }
        }
        TCKind::String => Value::String(d.get_string_value(SELF)?),
        TCKind::WString => Value::String(d.get_wstring_value(SELF)?),
        TCKind::Enum => {
            let Type::Enum(e) = ty else {
                return mismatch("an enum TypeCode for a value that is not an enum");
            };
            let v = d.get_int32_value(SELF)?;
            let position = e
                .literals
                .iter()
                .position(|l| l.value == v)
                .ok_or_else(|| Error::Mismatch(format!("no enumerator of value {v}")))?;
            Value::Enum(position as u32)
        }
        TCKind::Sequence => {
            if !matches!(ty, Type::Sequence(_)) {
                return mismatch("a sequence TypeCode for a value that is not a sequence");
            }
            let element = tc.content_type()?;
            let items = (0..d.get_item_count())
                .map(|i| to_value(d.get_value(i)?, &element))
                .collect::<Result<Vec<_>>>()?;
            list(items, &element)
        }
        TCKind::Array => {
            let Type::Array(a) = ty else {
                return mismatch("an array TypeCode for a value that is not an array");
            };
            // The TypeCodes of the arrays, outermost first, and their
            // elements.
            let mut levels = Vec::new();
            let mut element = tc.clone();
            for _ in &a.dimensions {
                if element.kind() != TCKind::Array {
                    return mismatch("an array of fewer dimensions");
                }
                levels.push(element.clone());
                element = element.content_type()?.unaliased();
            }
            let mut items = (0..d.get_item_count())
                .map(|i| to_value(d.get_value(i)?, &element))
                .collect::<Result<Vec<_>>>()?;
            // Innermost first: group the items into arrays of each level.
            for level in levels.iter().rev() {
                let len = level.length()? as usize;
                let element = level.content_type()?;
                if len == 0 || items.len() % len != 0 {
                    return mismatch("an array value of the wrong length");
                }
                let mut grouped = Vec::with_capacity(items.len() / len);
                let mut rest = items.into_iter();
                loop {
                    let chunk: Vec<Value> = rest.by_ref().take(len).collect();
                    if chunk.is_empty() {
                        break;
                    }
                    grouped.push(list(chunk, &element));
                }
                items = grouped;
            }
            items
                .pop()
                .filter(|_| items.is_empty())
                .ok_or_else(|| Error::Mismatch("an array value of the wrong length".into()))?
        }
        TCKind::Struct => {
            if !matches!(ty, Type::Struct(_)) {
                return mismatch("a struct TypeCode for a value that is not a struct");
            }
            let mut members = Vec::new();
            for i in 0..tc.member_count()? {
                let name = tc.member_name(i)?;
                let id = d.get_member_id_by_name(name);
                if id == MEMBER_ID_INVALID {
                    return mismatch(format!("no member {name}"));
                }
                members.push(to_value(d.get_value(id)?, &tc.member_type(i)?)?);
            }
            Value::List(members)
        }
        TCKind::Union => {
            let Type::Union(u) = ty else {
                return mismatch("a union TypeCode for a value that is not a union");
            };
            let x = d.get_discriminator_value()?;
            let disc_tc = tc.discriminator_type()?.unaliased();
            let int = |what: &str| Error::Mismatch(format!("the discriminator {x} as {what}"));
            let disc = match disc_tc.kind() {
                TCKind::Enum => {
                    let Type::Enum(e) = resolve(&u.discriminator) else {
                        return mismatch("an enum discriminator TypeCode for another type");
                    };
                    let p = e
                        .literals
                        .iter()
                        .position(|l| i64::from(l.value) == x)
                        .ok_or_else(|| int("an enumerator"))?;
                    Value::Enum(p as u32)
                }
                TCKind::Boolean => Value::Bool(x != 0),
                TCKind::Char => Value::Char(
                    u32::try_from(x)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| int("a char"))?,
                ),
                TCKind::WChar => Value::WChar(u16::try_from(x).map_err(|_| int("a wchar"))?),
                TCKind::Octet => Value::Octet(u8::try_from(x).map_err(|_| int("an octet"))?),
                TCKind::Short => Value::Short(i16::try_from(x).map_err(|_| int("a short"))?),
                TCKind::UShort => Value::UShort(u16::try_from(x).map_err(|_| int("a ushort"))?),
                TCKind::Long => Value::Long(i32::try_from(x).map_err(|_| int("a long"))?),
                TCKind::ULong => Value::ULong(u32::try_from(x).map_err(|_| int("a ulong"))?),
                TCKind::LongLong => Value::LongLong(x),
                TCKind::ULongLong => Value::ULongLong(x as u64),
                _ => return mismatch("a discriminator TypeCode of another kind"),
            };
            let member = match (tc.union_member_for(&disc)?, d.selected_member()) {
                (Some(index), Some(id)) => {
                    let name = tc.member_name(index)?;
                    if !u.members.iter().any(|m| m.id == id && m.name == name) {
                        return mismatch(format!("the selected member is not {name}"));
                    }
                    let value = to_value(d.get_value(id)?, &tc.member_type(index)?)?;
                    Some((index, Box::new(value)))
                }
                (None, None) => None,
                _ => return mismatch(format!("the discriminator {x} selects another member")),
            };
            Value::Union {
                disc: Box::new(disc),
                member,
            }
        }
        kind => return unsupported(format!("TypeCodes of kind {}", kind.name())),
    })
}
