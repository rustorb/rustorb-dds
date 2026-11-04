//! The [`Type`] that a TypeObject describes: the inverse of
//! [`TypeObjects::new`](crate::types::TypeObjects::new), for
//! `create_type_w_type_object` (7.5.2.2.8).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::registry::TypeRegistry;
use crate::type_object::*;
use crate::types::*;
use crate::{Error, Result};

/// The type of `object`, with the types it uses from `registry`.
pub(crate) fn type_of_object(object: &TypeObject, registry: &TypeRegistry) -> Result<Type> {
    Converter::new(registry).object(object, None)
}

/// The type `id` identifies.
pub(crate) fn type_of_identifier(id: &TypeIdentifier, registry: &TypeRegistry) -> Result<Type> {
    Converter::new(registry).identifier(id)
}

struct Converter<'a> {
    registry: &'a TypeRegistry,
    /// The named types converted, so that one used twice is shared.
    done: RefCell<HashMap<TypeIdentifier, Type>>,
    /// Those being converted, to find types that depend on each other.
    in_progress: RefCell<Vec<TypeIdentifier>>,
}

fn extensibility(flags: TypeFlag, default: Extensibility) -> Extensibility {
    if flags.contains(TypeFlag::IS_MUTABLE) {
        Extensibility::Mutable
    } else if flags.contains(TypeFlag::IS_APPENDABLE) {
        Extensibility::Appendable
    } else if flags.contains(TypeFlag::IS_FINAL) {
        Extensibility::Final
    } else {
        default
    }
}

fn try_construct(flags: MemberFlag) -> TryConstruct {
    match flags.0 & 3 {
        2 => TryConstruct::UseDefault,
        3 => TryConstruct::Trim,
        _ => TryConstruct::Discard,
    }
}

fn hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// The name a minimal TypeObject gives no name to: after its
/// TypeIdentifier.
fn minimal_name(id: Option<&TypeIdentifier>) -> String {
    match id {
        Some(TypeIdentifier::Minimal(h) | TypeIdentifier::Complete(h)) => {
            format!("minimal_{}", hex(h))
        }
        _ => "minimal".into(),
    }
}

impl<'a> Converter<'a> {
    fn new(registry: &'a TypeRegistry) -> Self {
        Converter {
            registry,
            done: RefCell::new(HashMap::new()),
            in_progress: RefCell::new(Vec::new()),
        }
    }

    fn element(&self, flags: MemberFlag, id: &TypeIdentifier) -> Result<Element> {
        Ok(Element {
            ty: self.identifier(id)?,
            external: flags.contains(MemberFlag::IS_EXTERNAL),
            try_construct: try_construct(flags),
        })
    }

    fn identifier(&self, id: &TypeIdentifier) -> Result<Type> {
        Ok(match id {
            TypeIdentifier::Primitive(k) => {
                super::DynamicTypeBuilderFactory::get_primitive_type(*k)
                    .ok_or(Error::Invalid("not a primitive type kind"))?
                    .into_type()
            }
            TypeIdentifier::StringSmall { wide, defn } => Type::String {
                wide: *wide,
                bound: u32::from(defn.bound),
            },
            TypeIdentifier::StringLarge { wide, defn } => Type::String {
                wide: *wide,
                bound: defn.bound,
            },
            TypeIdentifier::SeqSmall(d) => Type::Sequence(Arc::new(SequenceType {
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                bound: u32::from(d.bound),
            })),
            TypeIdentifier::SeqLarge(d) => Type::Sequence(Arc::new(SequenceType {
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                bound: d.bound,
            })),
            TypeIdentifier::ArraySmall(d) => Type::Array(Arc::new(ArrayType {
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                dimensions: d.array_bound_seq.iter().map(|b| u32::from(*b)).collect(),
            })),
            TypeIdentifier::ArrayLarge(d) => Type::Array(Arc::new(ArrayType {
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                dimensions: d.array_bound_seq.clone(),
            })),
            TypeIdentifier::MapSmall(d) => Type::Map(Arc::new(MapType {
                key: self.element(d.key_flags, &d.key_identifier)?,
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                bound: u32::from(d.bound),
            })),
            TypeIdentifier::MapLarge(d) => Type::Map(Arc::new(MapType {
                key: self.element(d.key_flags, &d.key_identifier)?,
                element: self.element(d.header.element_flags, &d.element_identifier)?,
                bound: d.bound,
            })),
            TypeIdentifier::Minimal(_) | TypeIdentifier::Complete(_) => {
                if let Some(t) = self.done.borrow().get(id) {
                    return Ok(t.clone());
                }
                if self.in_progress.borrow().contains(id) {
                    return Err(Error::Unsupported("types that depend on each other"));
                }
                let object = self
                    .registry
                    .get(id)
                    .ok_or(Error::Unsupported("a type whose TypeObject is not known"))?;
                self.in_progress.borrow_mut().push(id.clone());
                let ty = self.object(object, Some(id));
                self.in_progress.borrow_mut().pop();
                let ty = ty?;
                self.done.borrow_mut().insert(id.clone(), ty.clone());
                ty
            }
            TypeIdentifier::StronglyConnectedComponent(_) => {
                return Err(Error::Unsupported("types that depend on each other"));
            }
            TypeIdentifier::Extended(_) => {
                return Err(Error::Unsupported("an unknown TypeIdentifier kind"));
            }
        })
    }

    fn base(&self, id: &TypeIdentifier) -> Result<Option<Type>> {
        Ok(match id {
            TypeIdentifier::Primitive(TK_NONE) => None,
            other => Some(self.identifier(other)?),
        })
    }

    fn object(&self, object: &TypeObject, id: Option<&TypeIdentifier>) -> Result<Type> {
        match object {
            TypeObject::Complete(c) => self.complete(c),
            TypeObject::Minimal(m) => self.minimal(m, id),
        }
    }

    fn complete(&self, object: &CompleteTypeObject) -> Result<Type> {
        Ok(match object {
            CompleteTypeObject::Alias(t) => Type::from(AliasType {
                name: t.header.detail.type_name.clone(),
                related: self.identifier(&t.body.common.related_type)?,
            }),
            CompleteTypeObject::Struct(t) => Type::from(StructType {
                name: t.header.detail.type_name.clone(),
                extensibility: extensibility(t.struct_flags, Extensibility::Appendable),
                nested: t.struct_flags.contains(TypeFlag::IS_NESTED),
                autoid_hash: t.struct_flags.contains(TypeFlag::IS_AUTOID_HASH),
                base: self.base(&t.header.base_type)?,
                members: t
                    .member_seq
                    .iter()
                    .map(|m| {
                        self.struct_member(
                            &m.common,
                            m.detail.name.clone(),
                            m.detail.ann_builtin.clone().unwrap_or_default(),
                        )
                    })
                    .collect::<Result<_>>()?,
            }),
            CompleteTypeObject::Union(t) => Type::from(UnionType {
                name: t.header.detail.type_name.clone(),
                extensibility: extensibility(t.union_flags, Extensibility::Appendable),
                nested: t.union_flags.contains(TypeFlag::IS_NESTED),
                autoid_hash: t.union_flags.contains(TypeFlag::IS_AUTOID_HASH),
                discriminator: self.identifier(&t.discriminator.common.type_id)?,
                discriminator_key: t
                    .discriminator
                    .common
                    .member_flags
                    .contains(MemberFlag::IS_KEY),
                discriminator_try_construct: try_construct(t.discriminator.common.member_flags),
                members: t
                    .member_seq
                    .iter()
                    .map(|m| {
                        self.union_member(
                            &m.common,
                            m.detail.name.clone(),
                            m.detail.ann_builtin.clone().unwrap_or_default(),
                        )
                    })
                    .collect::<Result<_>>()?,
            }),
            CompleteTypeObject::Sequence(t) => Type::Sequence(Arc::new(SequenceType {
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                bound: t.header.common.bound,
            })),
            CompleteTypeObject::Array(t) => Type::Array(Arc::new(ArrayType {
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                dimensions: t.header.common.bound_seq.clone(),
            })),
            CompleteTypeObject::Map(t) => Type::Map(Arc::new(MapType {
                key: self.element(t.key.common.element_flags, &t.key.common.r#type)?,
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                bound: t.header.common.bound,
            })),
            CompleteTypeObject::Enumerated(t) => Type::from(EnumType {
                name: t.header.detail.type_name.clone(),
                bit_bound: t.header.common.bit_bound,
                extensibility: extensibility(t.enum_flags, Extensibility::Final),
                literals: t
                    .literal_seq
                    .iter()
                    .map(|l| EnumLiteral {
                        name: l.detail.name.clone(),
                        value: l.common.value,
                        default: l.common.flags.contains(MemberFlag::IS_DEFAULT),
                    })
                    .collect(),
            }),
            CompleteTypeObject::Bitmask(t) => Type::from(BitmaskType {
                name: t.header.detail.type_name.clone(),
                bit_bound: t.header.common.bit_bound,
                extensibility: extensibility(t.bitmask_flags, Extensibility::Final),
                flags: t
                    .flag_seq
                    .iter()
                    .map(|f| Bitflag {
                        name: f.detail.name.clone(),
                        position: f.common.position,
                    })
                    .collect(),
            }),
            CompleteTypeObject::Annotation(_) => {
                return Err(Error::Unsupported("annotation types"));
            }
            CompleteTypeObject::Bitset(_) => return Err(Error::Unsupported("bitset types")),
            CompleteTypeObject::Extended(_) => {
                return Err(Error::Unsupported("an unknown type kind"));
            }
        })
    }

    fn minimal(&self, object: &MinimalTypeObject, id: Option<&TypeIdentifier>) -> Result<Type> {
        let name = minimal_name(id);
        let member = |id: MemberId| format!("m{id}");
        let none = AppliedBuiltinMemberAnnotations::default;
        Ok(match object {
            MinimalTypeObject::Alias(t) => Type::from(AliasType {
                name,
                related: self.identifier(&t.body.common.related_type)?,
            }),
            MinimalTypeObject::Struct(t) => Type::from(StructType {
                name,
                extensibility: extensibility(t.struct_flags, Extensibility::Appendable),
                nested: t.struct_flags.contains(TypeFlag::IS_NESTED),
                autoid_hash: t.struct_flags.contains(TypeFlag::IS_AUTOID_HASH),
                base: self.base(&t.header.base_type)?,
                members: t
                    .member_seq
                    .iter()
                    .map(|m| self.struct_member(&m.common, member(m.common.member_id), none()))
                    .collect::<Result<_>>()?,
            }),
            MinimalTypeObject::Union(t) => Type::from(UnionType {
                name,
                extensibility: extensibility(t.union_flags, Extensibility::Appendable),
                nested: t.union_flags.contains(TypeFlag::IS_NESTED),
                autoid_hash: t.union_flags.contains(TypeFlag::IS_AUTOID_HASH),
                discriminator: self.identifier(&t.discriminator.common.type_id)?,
                discriminator_key: t
                    .discriminator
                    .common
                    .member_flags
                    .contains(MemberFlag::IS_KEY),
                discriminator_try_construct: try_construct(t.discriminator.common.member_flags),
                members: t
                    .member_seq
                    .iter()
                    .map(|m| self.union_member(&m.common, member(m.common.member_id), none()))
                    .collect::<Result<_>>()?,
            }),
            MinimalTypeObject::Sequence(t) => Type::Sequence(Arc::new(SequenceType {
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                bound: t.header.common.bound,
            })),
            MinimalTypeObject::Array(t) => Type::Array(Arc::new(ArrayType {
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                dimensions: t.header.common.bound_seq.clone(),
            })),
            MinimalTypeObject::Map(t) => Type::Map(Arc::new(MapType {
                key: self.element(t.key.common.element_flags, &t.key.common.r#type)?,
                element: self.element(t.element.common.element_flags, &t.element.common.r#type)?,
                bound: t.header.common.bound,
            })),
            MinimalTypeObject::Enumerated(t) => Type::from(EnumType {
                name,
                bit_bound: t.header.common.bit_bound,
                extensibility: extensibility(t.enum_flags, Extensibility::Final),
                literals: t
                    .literal_seq
                    .iter()
                    .map(|l| EnumLiteral {
                        name: format!("l{}", l.common.value),
                        value: l.common.value,
                        default: l.common.flags.contains(MemberFlag::IS_DEFAULT),
                    })
                    .collect(),
            }),
            MinimalTypeObject::Bitmask(t) => Type::from(BitmaskType {
                name,
                bit_bound: t.header.common.bit_bound,
                extensibility: extensibility(t.bitmask_flags, Extensibility::Final),
                flags: t
                    .flag_seq
                    .iter()
                    .map(|f| Bitflag {
                        name: format!("f{}", f.common.position),
                        position: f.common.position,
                    })
                    .collect(),
            }),
            MinimalTypeObject::Annotation(_) => {
                return Err(Error::Unsupported("annotation types"));
            }
            MinimalTypeObject::Bitset(_) => return Err(Error::Unsupported("bitset types")),
            MinimalTypeObject::Extended(_) => {
                return Err(Error::Unsupported("an unknown type kind"));
            }
        })
    }

    fn struct_member(
        &self,
        common: &CommonStructMember,
        name: String,
        annotations: AppliedBuiltinMemberAnnotations,
    ) -> Result<StructMember> {
        let f = common.member_flags;
        let key = f.contains(MemberFlag::IS_KEY);
        Ok(StructMember {
            name,
            id: common.member_id,
            ty: self.identifier(&common.member_type_id)?,
            key,
            optional: f.contains(MemberFlag::IS_OPTIONAL),
            // Key members are must-understand anyway.
            must_understand: f.contains(MemberFlag::IS_MUST_UNDERSTAND) && !key,
            external: f.contains(MemberFlag::IS_EXTERNAL),
            try_construct: try_construct(f),
            annotations,
        })
    }

    fn union_member(
        &self,
        common: &CommonUnionMember,
        name: String,
        annotations: AppliedBuiltinMemberAnnotations,
    ) -> Result<UnionMember> {
        let f = common.member_flags;
        Ok(UnionMember {
            name,
            id: common.member_id,
            ty: self.identifier(&common.type_id)?,
            labels: common.label_seq.clone(),
            default: f.contains(MemberFlag::IS_DEFAULT),
            external: f.contains(MemberFlag::IS_EXTERNAL),
            try_construct: try_construct(f),
            annotations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A type with every kind the conversion handles.
    fn sample() -> Type {
        let color: Type = EnumType {
            name: "M::Color".into(),
            literals: vec![
                EnumLiteral {
                    name: "RED".into(),
                    value: 0,
                    default: false,
                },
                EnumLiteral {
                    name: "BLUE".into(),
                    value: 5,
                    default: true,
                },
            ],
            ..EnumType::default()
        }
        .into();
        let perm: Type = BitmaskType {
            name: "M::Perm".into(),
            bit_bound: 8,
            flags: vec![Bitflag {
                name: "READ".into(),
                position: 2,
            }],
            ..BitmaskType::default()
        }
        .into();
        let inner: Type = StructType {
            name: "M::Inner".into(),
            extensibility: Extensibility::Final,
            members: vec![StructMember::new("x", 0, Type::Primitive(Primitive::Int16))],
            ..StructType::default()
        }
        .into();
        let derived: Type = StructType {
            name: "M::Derived".into(),
            base: Some(inner.clone()),
            members: vec![StructMember::new("y", 1, Type::Primitive(Primitive::Char8))],
            ..StructType::default()
        }
        .into();
        let alias: Type = AliasType {
            name: "M::Inners".into(),
            related: Type::sequence(inner.clone()),
        }
        .into();
        let u: Type = UnionType {
            name: "M::U".into(),
            extensibility: Extensibility::Mutable,
            discriminator: color.clone(),
            members: vec![
                UnionMember::new("a", 1, Type::Primitive(Primitive::Int64), vec![0]),
                UnionMember {
                    default: true,
                    ..UnionMember::new("b", 2, Type::string(), vec![])
                },
            ],
            ..UnionType::default()
        }
        .into();
        StructType {
            name: "M::All".into(),
            extensibility: Extensibility::Mutable,
            members: vec![
                StructMember {
                    key: true,
                    ..StructMember::new("id", 10, Type::Primitive(Primitive::Uint32))
                },
                StructMember {
                    optional: true,
                    ..StructMember::new("color", 11, color)
                },
                StructMember::new("perm", 12, perm),
                StructMember::new("inners", 13, alias),
                StructMember::new("derived", 14, derived),
                StructMember::new("u", 15, u),
                StructMember::new(
                    "grid",
                    16,
                    Type::Array(Arc::new(ArrayType {
                        element: Element::new(inner.clone()),
                        dimensions: vec![2, 300],
                    })),
                ),
                StructMember::new(
                    "map",
                    17,
                    Type::Map(Arc::new(MapType {
                        key: Element::new(Type::String {
                            wide: false,
                            bound: 8,
                        }),
                        element: Element::new(Type::String {
                            wide: true,
                            bound: 0,
                        }),
                        bound: 0,
                    })),
                ),
            ],
            ..StructType::default()
        }
        .into()
    }

    #[test]
    fn complete_type_objects_give_back_the_type() {
        let ty = sample();
        let objects = TypeObjects::new(&ty).unwrap();
        let mut registry = TypeRegistry::new();
        registry.add_type(&objects);
        let back = type_of_identifier(objects.complete_id(), &registry).unwrap();
        assert_eq!(back, ty);
        // And so the same TypeObjects.
        assert_eq!(TypeObjects::new(&back).unwrap(), objects);
    }

    #[test]
    fn minimal_type_objects_give_the_same_minimal_type() {
        let objects = TypeObjects::new(&sample()).unwrap();
        let mut registry = TypeRegistry::new();
        registry.add_type(&objects);
        let back = type_of_identifier(objects.minimal_id(), &registry).unwrap();
        let again = TypeObjects::new(&back).unwrap();
        // The names differ, but not what the minimal TypeObjects keep of
        // them... except the name hashes of the members, which are of
        // other names: compare the structure instead.
        let Type::Struct(s) = &back else { panic!() };
        assert_eq!(s.members.len(), 8);
        assert_eq!(s.members[0].name, "m10");
        assert!(s.members[0].key && s.members[1].optional);
        assert_eq!(again.minimal.len(), objects.minimal.len());
        // A type whose TypeObject is not known.
        assert!(type_of_identifier(&TypeIdentifier::Complete([1; 14]), &registry).is_err());
    }
}
