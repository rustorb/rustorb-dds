//! DynamicType and its builders (7.5.2.2 to 7.5.2.9).

use std::sync::Arc;

use crate::registry::TypeRegistry;
use crate::type_object::*;
use crate::types::*;
use crate::{Error, Result};

/// A type known at run time (`DynamicType`, 7.5.2.8): a [`Type`] of the
/// type system model, with the introspection operations of the
/// specification. Cloning is cheap; named types are shared.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicType(Type);

impl DynamicType {
    /// The DynamicType of `ty`.
    pub fn new(ty: Type) -> DynamicType {
        DynamicType(ty)
    }

    /// The type.
    pub fn as_type(&self) -> &Type {
        &self.0
    }

    /// The type.
    pub fn into_type(self) -> Type {
        self.0
    }

    /// `get_kind`.
    pub fn get_kind(&self) -> TypeKind {
        kind_of(&self.0)
    }

    /// `get_name`: the qualified name of a named type, empty for the
    /// others.
    pub fn get_name(&self) -> String {
        self.0.name().unwrap_or_default().to_owned()
    }

    /// `get_descriptor`.
    pub fn get_descriptor(&self) -> TypeDescriptor {
        let mut d = TypeDescriptor::new(self.get_kind(), &self.get_name());
        match &self.0 {
            Type::Primitive(_) => {}
            Type::String { bound, .. } => d.bound = vec![*bound],
            Type::Sequence(s) => {
                d.element_type = Some(DynamicType(s.element.ty.clone()));
                d.bound = vec![s.bound];
            }
            Type::Array(a) => {
                d.element_type = Some(DynamicType(a.element.ty.clone()));
                d.bound = a.dimensions.clone();
            }
            Type::Map(m) => {
                d.key_element_type = Some(DynamicType(m.key.ty.clone()));
                d.element_type = Some(DynamicType(m.element.ty.clone()));
                d.bound = vec![m.bound];
            }
            Type::Alias(a) => d.base_type = Some(DynamicType(a.related.clone())),
            Type::Enum(e) => {
                d.bound = vec![u32::from(e.bit_bound)];
                d.extensibility_kind = e.extensibility;
            }
            Type::Bitmask(b) => {
                d.bound = vec![u32::from(b.bit_bound)];
                d.extensibility_kind = b.extensibility;
            }
            Type::Struct(s) => {
                d.base_type = s.base.clone().map(DynamicType);
                d.extensibility_kind = s.extensibility;
                d.is_nested = s.nested;
            }
            Type::Union(u) => {
                d.discriminator_type = Some(DynamicType(u.discriminator.clone()));
                d.extensibility_kind = u.extensibility;
                d.is_nested = u.nested;
            }
        }
        d
    }

    /// `get_member_count`: the members of a struct (those of its base
    /// types first) or a union, the literals of an enum, the flags of a
    /// bitmask; 0 for the other types.
    pub fn get_member_count(&self) -> u32 {
        self.members().len() as u32
    }

    /// `get_member_by_index`.
    pub fn get_member_by_index(&self, index: u32) -> Option<DynamicTypeMember> {
        self.members().into_iter().nth(index as usize)
    }

    /// `get_member`: by member ID (for an enum, the value of a literal;
    /// for a bitmask, the position of a flag).
    pub fn get_member(&self, id: MemberId) -> Option<DynamicTypeMember> {
        self.members().into_iter().find(|m| m.0.id == id)
    }

    /// `get_member_by_name`.
    pub fn get_member_by_name(&self, name: &str) -> Option<DynamicTypeMember> {
        self.members().into_iter().find(|m| m.0.name == name)
    }

    /// Whether the type has key members (a struct with `@key` members,
    /// its base's included): its samples are of instances.
    pub fn is_keyed(&self) -> bool {
        match resolve(&self.0) {
            Type::Struct(s) => struct_members(s).iter().any(|m| m.key),
            _ => false,
        }
    }

    /// `get_all_members`, in order.
    pub fn members(&self) -> Vec<DynamicTypeMember> {
        let ty = resolve(&self.0);
        let out: Vec<MemberDescriptor> = match ty {
            Type::Struct(s) => struct_members(s)
                .into_iter()
                .enumerate()
                .map(|(i, m)| MemberDescriptor {
                    name: m.name.clone(),
                    id: m.id,
                    r#type: DynamicType(m.ty.clone()),
                    index: i as u32,
                    try_construct_kind: m.try_construct,
                    is_key: m.key,
                    is_optional: m.optional,
                    is_must_understand: m.must_understand || m.key,
                    is_shared: m.external,
                    ..MemberDescriptor::default()
                })
                .collect(),
            Type::Union(u) => u
                .members
                .iter()
                .enumerate()
                .map(|(i, m)| MemberDescriptor {
                    name: m.name.clone(),
                    id: m.id,
                    r#type: DynamicType(m.ty.clone()),
                    index: i as u32,
                    label: m.labels.clone(),
                    is_default_label: m.default,
                    try_construct_kind: m.try_construct,
                    is_shared: m.external,
                    ..MemberDescriptor::default()
                })
                .collect(),
            Type::Enum(e) => e
                .literals
                .iter()
                .enumerate()
                .map(|(i, l)| MemberDescriptor {
                    name: l.name.clone(),
                    id: l.value as MemberId,
                    r#type: DynamicType(ty.clone()),
                    index: i as u32,
                    is_default_label: l.default,
                    ..MemberDescriptor::default()
                })
                .collect(),
            Type::Bitmask(b) => b
                .flags
                .iter()
                .enumerate()
                .map(|(i, f)| MemberDescriptor {
                    name: f.name.clone(),
                    id: MemberId::from(f.position),
                    r#type: DynamicType(Type::Primitive(Primitive::Boolean)),
                    index: i as u32,
                    ..MemberDescriptor::default()
                })
                .collect(),
            _ => Vec::new(),
        };
        out.into_iter().map(DynamicTypeMember).collect()
    }

    /// `equals`.
    pub fn equals(&self, other: &DynamicType) -> bool {
        self == other
    }
}

impl From<Type> for DynamicType {
    fn from(ty: Type) -> DynamicType {
        DynamicType(ty)
    }
}

/// The type kind of `ty`.
pub(crate) fn kind_of(ty: &Type) -> TypeKind {
    match ty {
        Type::Primitive(p) => p.kind(),
        Type::String { wide: false, .. } => TK_STRING8,
        Type::String { wide: true, .. } => TK_STRING16,
        Type::Sequence(_) => TK_SEQUENCE,
        Type::Array(_) => TK_ARRAY,
        Type::Map(_) => TK_MAP,
        Type::Alias(_) => TK_ALIAS,
        Type::Enum(_) => TK_ENUM,
        Type::Bitmask(_) => TK_BITMASK,
        Type::Struct(_) => TK_STRUCTURE,
        Type::Union(_) => TK_UNION,
    }
}

/// `ty` with its aliases resolved.
pub(crate) fn resolve(mut ty: &Type) -> &Type {
    while let Type::Alias(a) = ty {
        ty = &a.related;
    }
    ty
}

/// The members of a struct, those of its base types first.
pub(crate) fn struct_members(s: &StructType) -> Vec<&StructMember> {
    let mut out = match s.base.as_ref().map(resolve) {
        Some(Type::Struct(base)) => struct_members(base),
        _ => Vec::new(),
    };
    out.extend(&s.members);
    out
}

/// A member of a DynamicType (`DynamicTypeMember`, 7.5.2.6).
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicTypeMember(MemberDescriptor);

impl DynamicTypeMember {
    /// `get_descriptor`.
    pub fn get_descriptor(&self) -> &MemberDescriptor {
        &self.0
    }

    /// `get_id`.
    pub fn get_id(&self) -> MemberId {
        self.0.id
    }

    /// `get_name`.
    pub fn get_name(&self) -> &str {
        &self.0.name
    }
}

/// `TypeDescriptor` (7.5.2.4): what a type is, as given to
/// [`DynamicTypeBuilderFactory::create_type`].
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDescriptor {
    /// The type kind (`TK_*`).
    pub kind: TypeKind,
    /// The qualified name, for named types.
    pub name: String,
    /// The aliased type of an alias, the base type of a struct.
    pub base_type: Option<DynamicType>,
    /// The discriminator type of a union.
    pub discriminator_type: Option<DynamicType>,
    /// The bound of a string, sequence or map (0 for none), the
    /// dimensions of an array, the bit bound of an enum or bitmask.
    pub bound: Vec<u32>,
    /// The element type of a collection.
    pub element_type: Option<DynamicType>,
    /// The key type of a map.
    pub key_element_type: Option<DynamicType>,
    /// The extensibility of a struct, union, enum or bitmask.
    pub extensibility_kind: Extensibility,
    /// `@nested`.
    pub is_nested: bool,
}

impl TypeDescriptor {
    /// A descriptor of kind `kind` named `name`, the rest unset.
    pub fn new(kind: TypeKind, name: &str) -> TypeDescriptor {
        TypeDescriptor {
            kind,
            name: name.to_owned(),
            base_type: None,
            discriminator_type: None,
            bound: Vec::new(),
            element_type: None,
            key_element_type: None,
            extensibility_kind: if matches!(kind, TK_STRUCTURE | TK_UNION) {
                Extensibility::Appendable
            } else {
                Extensibility::Final
            },
            is_nested: false,
        }
    }

    /// `is_consistent`: whether a type can be built from it.
    pub fn is_consistent(&self) -> bool {
        DynamicTypeBuilder::new(self.clone())
            .and_then(DynamicTypeBuilder::build)
            .is_ok()
    }
}

/// `MemberDescriptor` (7.5.2.7): a member of a struct or union, a literal
/// of an enum (whose value is the `id`) or a flag of a bitmask (whose
/// position is the `id`).
#[derive(Debug, Clone, PartialEq)]
pub struct MemberDescriptor {
    /// The name.
    pub name: String,
    /// The member ID.
    pub id: MemberId,
    /// The type.
    pub r#type: DynamicType,
    /// The default value, as text (not used yet).
    pub default_value: String,
    /// The position among the members.
    pub index: u32,
    /// The case labels of a union member.
    pub label: Vec<i32>,
    /// `@try_construct`.
    pub try_construct_kind: TryConstruct,
    /// `@key`.
    pub is_key: bool,
    /// `@optional`.
    pub is_optional: bool,
    /// `@must_understand`.
    pub is_must_understand: bool,
    /// `@external`.
    pub is_shared: bool,
    /// The `default` case of a union, or the `@default_literal` of an
    /// enum.
    pub is_default_label: bool,
}

impl Default for MemberDescriptor {
    fn default() -> Self {
        MemberDescriptor {
            name: String::new(),
            id: super::MEMBER_ID_INVALID,
            r#type: DynamicType(Type::Primitive(Primitive::Int32)),
            default_value: String::new(),
            index: 0,
            label: Vec::new(),
            try_construct_kind: TryConstruct::Discard,
            is_key: false,
            is_optional: false,
            is_must_understand: false,
            is_shared: false,
            is_default_label: false,
        }
    }
}

/// Makes [`DynamicTypeBuilder`]s (`DynamicTypeBuilderFactory`, 7.5.2.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct DynamicTypeBuilderFactory;

impl DynamicTypeBuilderFactory {
    /// `get_primitive_type`: `None` for a kind that is not primitive.
    pub fn get_primitive_type(kind: TypeKind) -> Option<DynamicType> {
        let p = match kind {
            TK_BOOLEAN => Primitive::Boolean,
            TK_BYTE => Primitive::Byte,
            TK_INT8 => Primitive::Int8,
            TK_UINT8 => Primitive::Uint8,
            TK_INT16 => Primitive::Int16,
            TK_UINT16 => Primitive::Uint16,
            TK_INT32 => Primitive::Int32,
            TK_UINT32 => Primitive::Uint32,
            TK_INT64 => Primitive::Int64,
            TK_UINT64 => Primitive::Uint64,
            TK_FLOAT32 => Primitive::Float32,
            TK_FLOAT64 => Primitive::Float64,
            TK_FLOAT128 => Primitive::Float128,
            TK_CHAR8 => Primitive::Char8,
            TK_CHAR16 => Primitive::Char16,
            _ => return None,
        };
        Some(DynamicType(Type::Primitive(p)))
    }

    /// `create_type`.
    pub fn create_type(descriptor: TypeDescriptor) -> Result<DynamicTypeBuilder> {
        DynamicTypeBuilder::new(descriptor)
    }

    /// `create_type_copy`.
    pub fn create_type_copy(ty: &DynamicType) -> Result<DynamicTypeBuilder> {
        let mut builder = DynamicTypeBuilder::new(ty.get_descriptor())?;
        if matches!(ty.get_kind(), TK_STRUCTURE) {
            // The base type's members stay in the base type.
            let skip = match &ty.0 {
                Type::Struct(s) => struct_members(s).len() - s.members.len(),
                _ => 0,
            };
            builder.members = ty.members().into_iter().skip(skip).map(|m| m.0).collect();
        } else {
            builder.members = ty.members().into_iter().map(|m| m.0).collect();
        }
        Ok(builder)
    }

    /// `create_type_w_type_object`: the type a TypeObject describes. The
    /// TypeObjects of the types it uses must be in `registry`. A COMPLETE
    /// TypeObject gives the names of the type and its members; a MINIMAL
    /// one only their hashes, so the type is named after its
    /// TypeIdentifier and the members `m<id>`.
    pub fn create_type_w_type_object(
        type_object: &TypeObject,
        registry: &TypeRegistry,
    ) -> Result<DynamicTypeBuilder> {
        let ty = super::convert::type_of_object(type_object, registry)?;
        Self::create_type_copy(&DynamicType(ty))
    }

    /// The type a TypeIdentifier identifies, with the TypeObjects in
    /// `registry` (see [`Self::create_type_w_type_object`]).
    pub fn create_type_w_type_identifier(
        id: &TypeIdentifier,
        registry: &TypeRegistry,
    ) -> Result<DynamicType> {
        super::convert::type_of_identifier(id, registry).map(DynamicType)
    }

    /// `create_string_type`: a `string` of at most `bound` characters (0
    /// for none).
    pub fn create_string_type(bound: u32) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_STRING8, "");
        d.bound = vec![bound];
        DynamicTypeBuilder::new(d)
    }

    /// `create_wstring_type`.
    pub fn create_wstring_type(bound: u32) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_STRING16, "");
        d.bound = vec![bound];
        DynamicTypeBuilder::new(d)
    }

    /// `create_sequence_type`.
    pub fn create_sequence_type(element: &DynamicType, bound: u32) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_SEQUENCE, "");
        d.element_type = Some(element.clone());
        d.bound = vec![bound];
        DynamicTypeBuilder::new(d)
    }

    /// `create_array_type`.
    pub fn create_array_type(element: &DynamicType, bound: &[u32]) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_ARRAY, "");
        d.element_type = Some(element.clone());
        d.bound = bound.to_vec();
        DynamicTypeBuilder::new(d)
    }

    /// `create_map_type`.
    pub fn create_map_type(
        key: &DynamicType,
        element: &DynamicType,
        bound: u32,
    ) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_MAP, "");
        d.key_element_type = Some(key.clone());
        d.element_type = Some(element.clone());
        d.bound = vec![bound];
        DynamicTypeBuilder::new(d)
    }

    /// `create_bitmask_type`: an anonymous bitmask of `bound` bits, whose
    /// flags are then added as members.
    pub fn create_bitmask_type(bound: u32) -> Result<DynamicTypeBuilder> {
        let mut d = TypeDescriptor::new(TK_BITMASK, "");
        d.bound = vec![bound];
        DynamicTypeBuilder::new(d)
    }
}

/// Builds a [`DynamicType`] (`DynamicTypeBuilder`, 7.5.2.9).
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicTypeBuilder {
    descriptor: TypeDescriptor,
    members: Vec<MemberDescriptor>,
}

fn bad(what: impl Into<String>) -> Error {
    Error::BadParameter(what.into())
}

impl DynamicTypeBuilder {
    fn new(descriptor: TypeDescriptor) -> Result<DynamicTypeBuilder> {
        let named = matches!(
            descriptor.kind,
            TK_STRUCTURE | TK_UNION | TK_ENUM | TK_ALIAS
        );
        if named && descriptor.name.is_empty() {
            return Err(bad("a named type without a name"));
        }
        Ok(DynamicTypeBuilder {
            descriptor,
            members: Vec::new(),
        })
    }

    /// `get_descriptor`.
    pub fn get_descriptor(&self) -> &TypeDescriptor {
        &self.descriptor
    }

    /// `get_kind`.
    pub fn get_kind(&self) -> TypeKind {
        self.descriptor.kind
    }

    /// `get_name`.
    pub fn get_name(&self) -> &str {
        &self.descriptor.name
    }

    /// `get_member_count`: the members added.
    pub fn get_member_count(&self) -> u32 {
        self.members.len() as u32
    }

    /// `add_member`: a member of a struct or union, a literal of an enum,
    /// a flag of a bitmask. Its index is its position. Fails for other
    /// kinds, or when the name or the ID is taken.
    pub fn add_member(&mut self, mut member: MemberDescriptor) -> Result<()> {
        if !matches!(
            self.descriptor.kind,
            TK_STRUCTURE | TK_UNION | TK_ENUM | TK_BITMASK
        ) {
            return Err(bad("this kind of type has no members"));
        }
        if member.name.is_empty() {
            return Err(bad("a member without a name"));
        }
        if self
            .members
            .iter()
            .any(|m| m.name == member.name || m.id == member.id)
        {
            return Err(bad(format!("member {} or its ID is taken", member.name)));
        }
        member.index = self.members.len() as u32;
        self.members.push(member);
        Ok(())
    }

    /// `build`.
    pub fn build(self) -> Result<DynamicType> {
        let d = self.descriptor;
        let one_bound = |default: u32| -> Result<u32> {
            match d.bound.as_slice() {
                [] => Ok(default),
                [b] => Ok(*b),
                _ => Err(bad("more than one bound")),
            }
        };
        let element = |t: &Option<DynamicType>| -> Result<Element> {
            t.as_ref()
                .map(|t| Element::new(t.0.clone()))
                .ok_or_else(|| bad("no element type"))
        };
        let bit_bound = |max: u32| -> Result<u16> {
            let b = one_bound(32)?;
            if b == 0 || b > max {
                return Err(bad("bit bound out of range"));
            }
            Ok(b as u16)
        };
        let ty = match d.kind {
            k if DynamicTypeBuilderFactory::get_primitive_type(k).is_some() => {
                DynamicTypeBuilderFactory::get_primitive_type(k)
                    .expect("primitive")
                    .0
            }
            TK_STRING8 | TK_STRING16 => Type::String {
                wide: d.kind == TK_STRING16,
                bound: one_bound(0)?,
            },
            TK_SEQUENCE => Type::Sequence(Arc::new(SequenceType {
                element: element(&d.element_type)?,
                bound: one_bound(0)?,
            })),
            TK_ARRAY => {
                if d.bound.is_empty() || d.bound.contains(&0) {
                    return Err(bad("an array needs dimensions"));
                }
                Type::Array(Arc::new(ArrayType {
                    element: element(&d.element_type)?,
                    dimensions: d.bound.clone(),
                }))
            }
            TK_MAP => Type::Map(Arc::new(MapType {
                key: element(&d.key_element_type)?,
                element: element(&d.element_type)?,
                bound: one_bound(0)?,
            })),
            TK_ALIAS => Type::from(AliasType {
                name: d.name.clone(),
                related: d
                    .base_type
                    .as_ref()
                    .map(|t| t.0.clone())
                    .ok_or_else(|| bad("an alias needs a base type"))?,
            }),
            TK_ENUM => Type::from(EnumType {
                name: d.name.clone(),
                bit_bound: bit_bound(32)?,
                extensibility: d.extensibility_kind,
                literals: self
                    .members
                    .iter()
                    .map(|m| EnumLiteral {
                        name: m.name.clone(),
                        value: m.id as i32,
                        default: m.is_default_label,
                    })
                    .collect(),
            }),
            TK_BITMASK => {
                let bits = bit_bound(64)?;
                if self.members.iter().any(|m| m.id >= u32::from(bits)) {
                    return Err(bad("a flag beyond the bit bound"));
                }
                Type::from(BitmaskType {
                    name: d.name.clone(),
                    bit_bound: bits,
                    extensibility: d.extensibility_kind,
                    flags: self
                        .members
                        .iter()
                        .map(|m| Bitflag {
                            name: m.name.clone(),
                            position: m.id as u16,
                        })
                        .collect(),
                })
            }
            TK_STRUCTURE => {
                let base = d.base_type.as_ref().map(|t| t.0.clone());
                if let Some(b) = &base {
                    if !matches!(resolve(b), Type::Struct(_)) {
                        return Err(bad("the base type of a struct is not a struct"));
                    }
                }
                Type::from(StructType {
                    name: d.name.clone(),
                    extensibility: d.extensibility_kind,
                    nested: d.is_nested,
                    autoid_hash: false,
                    base,
                    members: self
                        .members
                        .iter()
                        .map(|m| StructMember {
                            key: m.is_key,
                            optional: m.is_optional,
                            // A key member is must-understand anyway (7.2.2.4.4.4.8).
                            must_understand: m.is_must_understand && !m.is_key,
                            external: m.is_shared,
                            try_construct: m.try_construct_kind,
                            ..StructMember::new(m.name.clone(), m.id, m.r#type.0.clone())
                        })
                        .collect(),
                })
            }
            TK_UNION => {
                let discriminator = d
                    .discriminator_type
                    .as_ref()
                    .map(|t| t.0.clone())
                    .ok_or_else(|| bad("a union needs a discriminator type"))?;
                if !is_discriminator(resolve(&discriminator)) {
                    return Err(bad("not a discriminator type"));
                }
                Type::from(UnionType {
                    name: d.name.clone(),
                    extensibility: d.extensibility_kind,
                    nested: d.is_nested,
                    discriminator,
                    members: self
                        .members
                        .iter()
                        .map(|m| UnionMember {
                            default: m.is_default_label,
                            external: m.is_shared,
                            try_construct: m.try_construct_kind,
                            ..UnionMember::new(
                                m.name.clone(),
                                m.id,
                                m.r#type.0.clone(),
                                m.label.clone(),
                            )
                        })
                        .collect(),
                    ..UnionType::default()
                })
            }
            other => {
                return Err(Error::Unsupported(match other {
                    TK_ANNOTATION => "annotation types",
                    TK_BITSET => "bitset types",
                    _ => "this type kind",
                }));
            }
        };
        Ok(DynamicType(ty))
    }
}

/// Whether a union can be discriminated by `ty` (7.2.2.4.4.3).
fn is_discriminator(ty: &Type) -> bool {
    match ty {
        Type::Primitive(p) => !matches!(
            p,
            Primitive::Float32 | Primitive::Float64 | Primitive::Float128
        ),
        Type::Enum(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int32() -> DynamicType {
        DynamicTypeBuilderFactory::get_primitive_type(TK_INT32).unwrap()
    }

    #[test]
    fn builds_and_introspects_a_struct() {
        let mut b = DynamicTypeBuilderFactory::create_type(TypeDescriptor {
            extensibility_kind: Extensibility::Mutable,
            ..TypeDescriptor::new(TK_STRUCTURE, "M::S")
        })
        .unwrap();
        b.add_member(MemberDescriptor {
            name: "id".into(),
            id: 1,
            r#type: int32(),
            is_key: true,
            ..Default::default()
        })
        .unwrap();
        let text = DynamicTypeBuilderFactory::create_string_type(10)
            .unwrap()
            .build()
            .unwrap();
        b.add_member(MemberDescriptor {
            name: "text".into(),
            id: 2,
            r#type: text.clone(),
            is_optional: true,
            ..Default::default()
        })
        .unwrap();
        // Taken.
        assert!(
            b.add_member(MemberDescriptor {
                name: "text".into(),
                id: 3,
                ..Default::default()
            })
            .is_err()
        );
        let s = b.build().unwrap();
        assert_eq!(s.get_kind(), TK_STRUCTURE);
        assert_eq!(s.get_name(), "M::S");
        assert_eq!(s.get_member_count(), 2);
        let m = s.get_member_by_name("text").unwrap();
        assert_eq!(m.get_id(), 2);
        assert!(m.get_descriptor().is_optional);
        assert_eq!(m.get_descriptor().r#type, text);
        assert!(s.get_member(1).unwrap().get_descriptor().is_key);
        assert_eq!(
            s.get_descriptor().extensibility_kind,
            Extensibility::Mutable
        );
        // A copy builds the same type.
        let copy = DynamicTypeBuilderFactory::create_type_copy(&s)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(copy, s);
    }

    #[test]
    fn other_kinds() {
        let seq = DynamicTypeBuilderFactory::create_sequence_type(&int32(), 5)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(seq.get_descriptor().bound, [5]);
        assert_eq!(seq.get_descriptor().element_type, Some(int32()));
        let arr = DynamicTypeBuilderFactory::create_array_type(&seq, &[2, 3])
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(arr.get_kind(), TK_ARRAY);
        assert!(
            DynamicTypeBuilderFactory::create_array_type(&seq, &[])
                .unwrap()
                .build()
                .is_err()
        );
        let mut e =
            DynamicTypeBuilderFactory::create_type(TypeDescriptor::new(TK_ENUM, "M::E")).unwrap();
        for (i, n) in ["A", "B"].iter().enumerate() {
            e.add_member(MemberDescriptor {
                name: (*n).into(),
                id: i as u32 * 10,
                ..Default::default()
            })
            .unwrap();
        }
        let e = e.build().unwrap();
        assert_eq!(e.get_member(10).unwrap().get_name(), "B");
        let mut u = DynamicTypeBuilderFactory::create_type(TypeDescriptor {
            discriminator_type: Some(e.clone()),
            ..TypeDescriptor::new(TK_UNION, "M::U")
        })
        .unwrap();
        u.add_member(MemberDescriptor {
            name: "a".into(),
            id: 1,
            r#type: int32(),
            label: vec![0],
            ..Default::default()
        })
        .unwrap();
        let u = u.build().unwrap();
        assert_eq!(
            u.get_member_by_index(0).unwrap().get_descriptor().label,
            [0]
        );
        // A float cannot discriminate.
        let float = DynamicTypeBuilderFactory::get_primitive_type(TK_FLOAT32);
        assert!(
            DynamicTypeBuilderFactory::create_type(TypeDescriptor {
                discriminator_type: float,
                ..TypeDescriptor::new(TK_UNION, "M::V")
            })
            .unwrap()
            .build()
            .is_err()
        );
        assert!(
            DynamicTypeBuilderFactory::create_type(TypeDescriptor::new(TK_STRUCTURE, "")).is_err()
        );
        assert!(DynamicTypeBuilderFactory::get_primitive_type(TK_STRUCTURE).is_none());
    }
}
