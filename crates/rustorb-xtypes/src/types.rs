//! A description of a type, after the type system model of DDS-XTypes
//! 1.3 clause 7.2.2, and the TypeObjects, TypeIdentifiers and
//! TypeInformation built from it (7.3.4, 7.6.3.2.1).
//!
//! A [`Type`] says everything that goes into a TypeObject: names,
//! extensibility, member IDs and flags. It does not choose member IDs or
//! default extensibility; that is up to whoever reads the IDL.
//!
//! ```
//! use rustorb_xtypes::types::*;
//!
//! // module HelloWorld { @final struct Greeting {
//! //     unsigned long index; string message; }; };
//! let greeting = Type::Struct(StructType {
//!     name: "HelloWorld::Greeting".into(),
//!     extensibility: Extensibility::Final,
//!     members: vec![
//!         StructMember::new("index", 0, Type::Primitive(Primitive::Uint32)),
//!         StructMember::new("message", 1, Type::string()),
//!     ],
//!     ..StructType::default()
//! }.into());
//! let objects = TypeObjects::new(&greeting).unwrap();
//! assert_eq!(objects.information.minimal.typeid_with_size.typeobject_serialized_size, 56);
//! ```
//!
//! Types that depend on each other (a struct with a sequence of itself,
//! 7.3.4.8) are not supported yet: [`TypeObjects::new`] returns
//! [`Error::Unsupported`].

use std::collections::HashMap;
use std::sync::Arc;

pub use rustorb_cdr::xcdr::Extensibility;

use crate::type_object::*;
use crate::{Error, Result};

/// A primitive type (7.2.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Primitive {
    Boolean,
    Byte,
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Float32,
    Float64,
    Float128,
    Char8,
    Char16,
}

impl Primitive {
    /// The type kind.
    pub fn kind(self) -> TypeKind {
        match self {
            Primitive::Boolean => TK_BOOLEAN,
            Primitive::Byte => TK_BYTE,
            Primitive::Int8 => TK_INT8,
            Primitive::Uint8 => TK_UINT8,
            Primitive::Int16 => TK_INT16,
            Primitive::Uint16 => TK_UINT16,
            Primitive::Int32 => TK_INT32,
            Primitive::Uint32 => TK_UINT32,
            Primitive::Int64 => TK_INT64,
            Primitive::Uint64 => TK_UINT64,
            Primitive::Float32 => TK_FLOAT32,
            Primitive::Float64 => TK_FLOAT64,
            Primitive::Float128 => TK_FLOAT128,
            Primitive::Char8 => TK_CHAR8,
            Primitive::Char16 => TK_CHAR16,
        }
    }
}

/// A type.
///
/// Named types are behind an [`Arc`] so that a type used in several
/// places is described once.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// A primitive type.
    Primitive(Primitive),
    /// `string<bound>` or `wstring<bound>`; a bound of 0 is unbounded.
    String {
        /// `wstring`.
        wide: bool,
        /// The bound, 0 for none.
        bound: u32,
    },
    /// An anonymous `sequence`.
    Sequence(Arc<SequenceType>),
    /// An anonymous array.
    Array(Arc<ArrayType>),
    /// An anonymous `map`.
    Map(Arc<MapType>),
    /// A `typedef`.
    Alias(Arc<AliasType>),
    /// An `enum`.
    Enum(Arc<EnumType>),
    /// A `bitmask`.
    Bitmask(Arc<BitmaskType>),
    /// A `struct`.
    Struct(Arc<StructType>),
    /// A `union`.
    Union(Arc<UnionType>),
}

impl Type {
    /// An unbounded `string`.
    pub fn string() -> Type {
        Type::String {
            wide: false,
            bound: 0,
        }
    }

    /// An unbounded `sequence` of `element`.
    pub fn sequence(element: Type) -> Type {
        Type::Sequence(Arc::new(SequenceType {
            element: Element::new(element),
            bound: 0,
        }))
    }

    /// The qualified name of a named type.
    pub fn name(&self) -> Option<&str> {
        match self {
            Type::Alias(t) => Some(&t.name),
            Type::Enum(t) => Some(&t.name),
            Type::Bitmask(t) => Some(&t.name),
            Type::Struct(t) => Some(&t.name),
            Type::Union(t) => Some(&t.name),
            _ => None,
        }
    }
}

impl From<StructType> for Type {
    fn from(t: StructType) -> Type {
        Type::Struct(Arc::new(t))
    }
}

impl From<UnionType> for Type {
    fn from(t: UnionType) -> Type {
        Type::Union(Arc::new(t))
    }
}

impl From<EnumType> for Type {
    fn from(t: EnumType) -> Type {
        Type::Enum(Arc::new(t))
    }
}

impl From<BitmaskType> for Type {
    fn from(t: BitmaskType) -> Type {
        Type::Bitmask(Arc::new(t))
    }
}

impl From<AliasType> for Type {
    fn from(t: AliasType) -> Type {
        Type::Alias(Arc::new(t))
    }
}

/// `@try_construct` (7.2.2.7): what a reader does with a member or element
/// it cannot construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TryConstruct {
    /// `DISCARD`, the default.
    #[default]
    Discard,
    /// `USE_DEFAULT`.
    UseDefault,
    /// `TRIM`.
    Trim,
}

impl TryConstruct {
    fn flags(self) -> MemberFlag {
        match self {
            TryConstruct::Discard => MemberFlag::TRY_CONSTRUCT_DISCARD,
            TryConstruct::UseDefault => MemberFlag::TRY_CONSTRUCT_USE_DEFAULT,
            TryConstruct::Trim => MemberFlag::TRY_CONSTRUCT_TRIM,
        }
    }
}

/// The element (or key) of a collection.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The element type.
    pub ty: Type,
    /// `@external`.
    pub external: bool,
    /// `@try_construct`.
    pub try_construct: TryConstruct,
}

impl Element {
    /// An element of type `ty`, with no annotations.
    pub fn new(ty: Type) -> Element {
        Element {
            ty,
            external: false,
            try_construct: TryConstruct::Discard,
        }
    }

    fn flags(&self) -> MemberFlag {
        let mut flags = self.try_construct.flags();
        if self.external {
            flags |= MemberFlag::IS_EXTERNAL;
        }
        flags
    }
}

/// `sequence<element, bound>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceType {
    /// The element.
    pub element: Element,
    /// The bound, 0 for none.
    pub bound: u32,
}

/// An array: `element[d0][d1]...`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType {
    /// The element.
    pub element: Element,
    /// The dimensions, outermost first.
    pub dimensions: Vec<u32>,
}

/// `map<key, element, bound>`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapType {
    /// The key.
    pub key: Element,
    /// The element.
    pub element: Element,
    /// The bound, 0 for none.
    pub bound: u32,
}

/// `typedef related name;`.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasType {
    /// The qualified name.
    pub name: String,
    /// The type it names.
    pub related: Type,
}

/// A literal of an [`EnumType`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EnumLiteral {
    /// The name.
    pub name: String,
    /// The value.
    pub value: i32,
    /// `@default_literal`.
    pub default: bool,
}

/// An `enum`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumType {
    /// The qualified name.
    pub name: String,
    /// `@bit_bound`, 32 by default.
    pub bit_bound: u16,
    /// The extensibility.
    pub extensibility: Extensibility,
    /// The literals.
    pub literals: Vec<EnumLiteral>,
}

impl Default for EnumType {
    fn default() -> Self {
        EnumType {
            name: String::new(),
            bit_bound: 32,
            extensibility: Extensibility::Final,
            literals: Vec::new(),
        }
    }
}

/// A flag of a [`BitmaskType`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bitflag {
    /// The name.
    pub name: String,
    /// The bit.
    pub position: u16,
}

/// A `bitmask`.
#[derive(Debug, Clone, PartialEq)]
pub struct BitmaskType {
    /// The qualified name.
    pub name: String,
    /// `@bit_bound`, 32 by default.
    pub bit_bound: u16,
    /// The extensibility.
    pub extensibility: Extensibility,
    /// The flags.
    pub flags: Vec<Bitflag>,
}

impl Default for BitmaskType {
    fn default() -> Self {
        BitmaskType {
            name: String::new(),
            bit_bound: 32,
            extensibility: Extensibility::Final,
            flags: Vec::new(),
        }
    }
}

/// A member of a [`StructType`].
#[derive(Debug, Clone, PartialEq)]
pub struct StructMember {
    /// The name.
    pub name: String,
    /// The member ID.
    pub id: MemberId,
    /// The type.
    pub ty: Type,
    /// `@key`. Key members are also must-understand.
    pub key: bool,
    /// `@optional`.
    pub optional: bool,
    /// `@must_understand`.
    pub must_understand: bool,
    /// `@external`.
    pub external: bool,
    /// `@try_construct`.
    pub try_construct: TryConstruct,
    /// `@unit`, `@min`, `@max`, `@range`, `@hashid`.
    pub annotations: AppliedBuiltinMemberAnnotations,
}

impl StructMember {
    /// A member with no annotations.
    pub fn new(name: impl Into<String>, id: MemberId, ty: Type) -> StructMember {
        StructMember {
            name: name.into(),
            id,
            ty,
            key: false,
            optional: false,
            must_understand: false,
            external: false,
            try_construct: TryConstruct::Discard,
            annotations: AppliedBuiltinMemberAnnotations::default(),
        }
    }

    fn flags(&self) -> MemberFlag {
        let mut flags = self.try_construct.flags();
        for (on, flag) in [
            (self.external, MemberFlag::IS_EXTERNAL),
            (self.optional, MemberFlag::IS_OPTIONAL),
            (
                self.must_understand || self.key,
                MemberFlag::IS_MUST_UNDERSTAND,
            ),
            (self.key, MemberFlag::IS_KEY),
        ] {
            if on {
                flags |= flag;
            }
        }
        flags
    }
}

/// A `struct`.
#[derive(Debug, Clone, PartialEq)]
pub struct StructType {
    /// The qualified name.
    pub name: String,
    /// The extensibility.
    pub extensibility: Extensibility,
    /// `@nested`.
    pub nested: bool,
    /// `@autoid(HASH)`.
    pub autoid_hash: bool,
    /// The base type, a struct or an alias of one.
    pub base: Option<Type>,
    /// The members, without those of the base type, in declaration order.
    pub members: Vec<StructMember>,
}

impl Default for StructType {
    fn default() -> Self {
        StructType {
            name: String::new(),
            extensibility: Extensibility::Appendable,
            nested: false,
            autoid_hash: false,
            base: None,
            members: Vec::new(),
        }
    }
}

/// A member (a case) of a [`UnionType`].
#[derive(Debug, Clone, PartialEq)]
pub struct UnionMember {
    /// The name.
    pub name: String,
    /// The member ID.
    pub id: MemberId,
    /// The type.
    pub ty: Type,
    /// The case labels, as `long`s (enum labels by their value).
    pub labels: Vec<i32>,
    /// Whether this is the `default` case.
    pub default: bool,
    /// `@external`.
    pub external: bool,
    /// `@try_construct`.
    pub try_construct: TryConstruct,
    /// `@unit`, `@min`, `@max`, `@range`, `@hashid`.
    pub annotations: AppliedBuiltinMemberAnnotations,
}

impl UnionMember {
    /// A member for `labels`, with no annotations.
    pub fn new(name: impl Into<String>, id: MemberId, ty: Type, labels: Vec<i32>) -> UnionMember {
        UnionMember {
            name: name.into(),
            id,
            ty,
            labels,
            default: false,
            external: false,
            try_construct: TryConstruct::Discard,
            annotations: AppliedBuiltinMemberAnnotations::default(),
        }
    }
}

/// A `union`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionType {
    /// The qualified name.
    pub name: String,
    /// The extensibility.
    pub extensibility: Extensibility,
    /// `@nested`.
    pub nested: bool,
    /// `@autoid(HASH)`.
    pub autoid_hash: bool,
    /// The discriminator type.
    pub discriminator: Type,
    /// `@key` on the discriminator.
    pub discriminator_key: bool,
    /// `@try_construct` on the discriminator.
    pub discriminator_try_construct: TryConstruct,
    /// The members, in declaration order.
    pub members: Vec<UnionMember>,
}

impl Default for UnionType {
    fn default() -> Self {
        UnionType {
            name: String::new(),
            extensibility: Extensibility::Appendable,
            nested: false,
            autoid_hash: false,
            discriminator: Type::Primitive(Primitive::Int32),
            discriminator_key: false,
            discriminator_try_construct: TryConstruct::Discard,
            members: Vec::new(),
        }
    }
}

/// A Rust type that knows its DDS-XTypes description. rustorb-idl writes
/// impls of it with `-Wbxtypes`.
pub trait DescribeType {
    /// The description of the type. Calls return the same description,
    /// sharing its named types.
    fn describe_type() -> Type;
}

// ---------- Building TypeObjects ---------------------------------------

/// The TypeObjects of a type and of the types it depends on, and the
/// TypeInformation that announces them.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeObjects {
    /// The minimal TypeObjects: the type's first, then those it depends
    /// on, in the order they are first used.
    pub minimal: Vec<TypeIdentifierTypeObjectPair>,
    /// The complete TypeObjects, in the same order.
    pub complete: Vec<TypeIdentifierTypeObjectPair>,
    /// The TypeInformation, with every type it depends on.
    pub information: TypeInformation,
}

impl TypeObjects {
    /// Builds the TypeObjects of `ty`, which must be a named type other
    /// than an alias (in practice a struct or a union: a topic type).
    pub fn new(ty: &Type) -> Result<TypeObjects> {
        let minimal = Builder::new(EK_MINIMAL).run(ty)?;
        let complete = Builder::new(EK_COMPLETE).run(ty)?;
        let information = TypeInformation {
            minimal: with_dependencies(&minimal)?,
            complete: with_dependencies(&complete)?,
        };
        Ok(TypeObjects {
            minimal,
            complete,
            information,
        })
    }

    /// Builds the TypeObjects of `T`.
    pub fn of<T: DescribeType>() -> Result<TypeObjects> {
        TypeObjects::new(&T::describe_type())
    }

    /// The minimal TypeIdentifier of the type.
    pub fn minimal_id(&self) -> &TypeIdentifier {
        &self.information.minimal.typeid_with_size.type_id
    }

    /// The complete TypeIdentifier of the type.
    pub fn complete_id(&self) -> &TypeIdentifier {
        &self.information.complete.typeid_with_size.type_id
    }

    /// The TypeObject that `id` identifies, if it is one of these.
    pub fn get(&self, id: &TypeIdentifier) -> Option<&TypeObject> {
        self.minimal
            .iter()
            .chain(&self.complete)
            .find(|p| &p.type_identifier == id)
            .map(|p| &p.type_object)
    }
}

fn with_dependencies(
    pairs: &[TypeIdentifierTypeObjectPair],
) -> Result<TypeIdentifierWithDependencies> {
    let sized = |p: &TypeIdentifierTypeObjectPair| -> Result<TypeIdentifierWithSize> {
        let size = p.type_object.to_bytes()?.len();
        Ok(TypeIdentifierWithSize {
            type_id: p.type_identifier.clone(),
            typeobject_serialized_size: u32::try_from(size)
                .map_err(|_| Error::InvalidType("TypeObject too large".into()))?,
        })
    };
    let (first, rest) = pairs
        .split_first()
        .ok_or_else(|| Error::InvalidType("no TypeObject".into()))?;
    let dependent_typeids = rest.iter().map(sized).collect::<Result<Vec<_>>>()?;
    Ok(TypeIdentifierWithDependencies {
        typeid_with_size: sized(first)?,
        dependent_typeid_count: i32::try_from(dependent_typeids.len())
            .map_err(|_| Error::InvalidType("too many types".into()))?,
        dependent_typeids,
    })
}

/// Computes TypeIdentifiers of one equivalence kind, collecting the
/// TypeObjects of the named types on the way.
struct Builder {
    kind: EquivalenceKind,
    /// Named types by address: their index in `objects`.
    done: HashMap<usize, usize>,
    /// Named types being built, to find types that depend on each other.
    in_progress: Vec<usize>,
    /// In the order the types were first used; filled in when built.
    objects: Vec<Option<TypeIdentifierTypeObjectPair>>,
}

fn address(ty: &Type) -> Option<usize> {
    Some(match ty {
        Type::Alias(t) => Arc::as_ptr(t) as usize,
        Type::Enum(t) => Arc::as_ptr(t) as usize,
        Type::Bitmask(t) => Arc::as_ptr(t) as usize,
        Type::Struct(t) => Arc::as_ptr(t) as usize,
        Type::Union(t) => Arc::as_ptr(t) as usize,
        _ => return None,
    })
}

impl Builder {
    fn new(kind: EquivalenceKind) -> Builder {
        Builder {
            kind,
            done: HashMap::new(),
            in_progress: Vec::new(),
            objects: Vec::new(),
        }
    }

    fn run(mut self, ty: &Type) -> Result<Vec<TypeIdentifierTypeObjectPair>> {
        if address(ty).is_none() {
            return Err(Error::InvalidType(
                "only a named type has a TypeObject".into(),
            ));
        }
        self.identifier(ty)?;
        Ok(self
            .objects
            .into_iter()
            .map(|p| p.expect("built"))
            .collect())
    }

    fn identifier(&mut self, ty: &Type) -> Result<TypeIdentifier> {
        Ok(match ty {
            Type::Primitive(p) => TypeIdentifier::Primitive(p.kind()),
            Type::String { wide, bound } => string_identifier(*wide, *bound),
            Type::Sequence(s) => {
                let element = self.identifier(&s.element.ty)?;
                let header = self.collection_header(&s.element, &[&element]);
                match u8::try_from(s.bound) {
                    Ok(bound) => TypeIdentifier::SeqSmall(PlainSequenceSElemDefn {
                        header,
                        bound,
                        element_identifier: Box::new(element),
                    }),
                    Err(_) => TypeIdentifier::SeqLarge(PlainSequenceLElemDefn {
                        header,
                        bound: s.bound,
                        element_identifier: Box::new(element),
                    }),
                }
            }
            Type::Array(a) => {
                if a.dimensions.is_empty() || a.dimensions.contains(&0) {
                    return Err(Error::InvalidType("array without dimensions".into()));
                }
                let element = self.identifier(&a.element.ty)?;
                let header = self.collection_header(&a.element, &[&element]);
                let small: Option<Vec<u8>> =
                    a.dimensions.iter().map(|&d| u8::try_from(d).ok()).collect();
                match small {
                    Some(array_bound_seq) => TypeIdentifier::ArraySmall(PlainArraySElemDefn {
                        header,
                        array_bound_seq,
                        element_identifier: Box::new(element),
                    }),
                    None => TypeIdentifier::ArrayLarge(PlainArrayLElemDefn {
                        header,
                        array_bound_seq: a.dimensions.clone(),
                        element_identifier: Box::new(element),
                    }),
                }
            }
            Type::Map(m) => {
                let key = self.identifier(&m.key.ty)?;
                let element = self.identifier(&m.element.ty)?;
                let header = self.collection_header(&m.element, &[&key, &element]);
                let key_flags = m.key.flags();
                match u8::try_from(m.bound) {
                    Ok(bound) => TypeIdentifier::MapSmall(PlainMapSTypeDefn {
                        header,
                        bound,
                        element_identifier: Box::new(element),
                        key_flags,
                        key_identifier: Box::new(key),
                    }),
                    Err(_) => TypeIdentifier::MapLarge(PlainMapLTypeDefn {
                        header,
                        bound: m.bound,
                        element_identifier: Box::new(element),
                        key_flags,
                        key_identifier: Box::new(key),
                    }),
                }
            }
            named => {
                let address = address(named).expect("a named type");
                if let Some(&i) = self.done.get(&address) {
                    return Ok(self.objects[i]
                        .as_ref()
                        .expect("built")
                        .type_identifier
                        .clone());
                }
                if self.in_progress.contains(&address) {
                    return Err(Error::Unsupported(
                        "types that depend on each other (DDS-XTypes 7.3.4.8)",
                    ));
                }
                let index = self.objects.len();
                self.objects.push(None);
                self.in_progress.push(address);
                let type_object = self.type_object(named)?;
                self.in_progress.pop();
                let type_identifier = type_object.type_identifier()?;
                self.objects[index] = Some(TypeIdentifierTypeObjectPair {
                    type_identifier: type_identifier.clone(),
                    type_object,
                });
                self.done.insert(address, index);
                type_identifier
            }
        })
    }

    fn collection_header(
        &self,
        element: &Element,
        identifiers: &[&TypeIdentifier],
    ) -> PlainCollectionHeader {
        let equiv_kind = if identifiers.iter().all(|i| i.is_fully_descriptive()) {
            EK_BOTH
        } else {
            self.kind
        };
        PlainCollectionHeader {
            equiv_kind,
            element_flags: element.flags(),
        }
    }

    fn complete(&self) -> bool {
        self.kind == EK_COMPLETE
    }

    fn type_object(&mut self, ty: &Type) -> Result<TypeObject> {
        match ty {
            Type::Alias(t) => self.alias(t),
            Type::Enum(t) => self.enumeration(t),
            Type::Bitmask(t) => self.bitmask(t),
            Type::Struct(t) => self.structure(t),
            Type::Union(t) => self.union(t),
            _ => unreachable!("only named types have TypeObjects"),
        }
    }

    fn alias(&mut self, t: &AliasType) -> Result<TypeObject> {
        let common = CommonAliasBody {
            related_flags: MemberFlag::default(),
            related_type: self.identifier(&t.related)?,
        };
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Alias(CompleteAliasType {
                alias_flags: TypeFlag::default(),
                header: CompleteAliasHeader {
                    detail: type_detail(&t.name),
                },
                body: CompleteAliasBody {
                    common,
                    ann_builtin: None,
                    ann_custom: None,
                },
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Alias(MinimalAliasType {
                alias_flags: TypeFlag::default(),
                header: MinimalAliasHeader {},
                body: MinimalAliasBody { common },
            }))
        })
    }

    fn enumeration(&mut self, t: &EnumType) -> Result<TypeObject> {
        let mut literals: Vec<&EnumLiteral> = t.literals.iter().collect();
        literals.sort_by_key(|l| l.value);
        let common = |l: &EnumLiteral| CommonEnumeratedLiteral {
            value: l.value,
            flags: if l.default {
                MemberFlag::IS_DEFAULT
            } else {
                MemberFlag::default()
            },
        };
        let header = CommonEnumeratedHeader {
            bit_bound: t.bit_bound,
        };
        let enum_flags = extensibility_flags(t.extensibility);
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Enumerated(CompleteEnumeratedType {
                enum_flags,
                header: CompleteEnumeratedHeader {
                    common: header,
                    detail: type_detail(&t.name),
                },
                literal_seq: literals
                    .into_iter()
                    .map(|l| CompleteEnumeratedLiteral {
                        common: common(l),
                        detail: member_detail(&l.name, None),
                    })
                    .collect(),
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Enumerated(MinimalEnumeratedType {
                enum_flags,
                header: MinimalEnumeratedHeader { common: header },
                literal_seq: literals
                    .into_iter()
                    .map(|l| MinimalEnumeratedLiteral {
                        common: common(l),
                        detail: MinimalMemberDetail {
                            name_hash: name_hash(&l.name),
                        },
                    })
                    .collect(),
            }))
        })
    }

    fn bitmask(&mut self, t: &BitmaskType) -> Result<TypeObject> {
        let mut flags: Vec<&Bitflag> = t.flags.iter().collect();
        flags.sort_by_key(|f| f.position);
        let common = |f: &Bitflag| CommonBitflag {
            position: f.position,
            flags: MemberFlag::default(),
        };
        let header = CommonEnumeratedHeader {
            bit_bound: t.bit_bound,
        };
        let bitmask_flags = extensibility_flags(t.extensibility);
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Bitmask(CompleteBitmaskType {
                bitmask_flags,
                header: CompleteEnumeratedHeader {
                    common: header,
                    detail: type_detail(&t.name),
                },
                flag_seq: flags
                    .into_iter()
                    .map(|f| CompleteBitflag {
                        common: common(f),
                        detail: member_detail(&f.name, None),
                    })
                    .collect(),
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Bitmask(MinimalBitmaskType {
                bitmask_flags,
                header: MinimalEnumeratedHeader { common: header },
                flag_seq: flags
                    .into_iter()
                    .map(|f| MinimalBitflag {
                        common: common(f),
                        detail: MinimalMemberDetail {
                            name_hash: name_hash(&f.name),
                        },
                    })
                    .collect(),
            }))
        })
    }

    fn structure(&mut self, t: &StructType) -> Result<TypeObject> {
        let base_type = match &t.base {
            Some(base) => self.identifier(base)?,
            None => TypeIdentifier::Primitive(TK_NONE),
        };
        let struct_flags = aggregate_flags(t.extensibility, t.nested, t.autoid_hash);
        let mut commons = Vec::with_capacity(t.members.len());
        for m in &t.members {
            commons.push(CommonStructMember {
                member_id: m.id,
                member_flags: m.flags(),
                member_type_id: self.identifier(&m.ty)?,
            });
        }
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Struct(CompleteStructType {
                struct_flags,
                header: CompleteStructHeader {
                    base_type,
                    detail: type_detail(&t.name),
                },
                member_seq: t
                    .members
                    .iter()
                    .zip(commons)
                    .map(|(m, common)| CompleteStructMember {
                        common,
                        detail: member_detail(&m.name, Some(&m.annotations)),
                    })
                    .collect(),
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Struct(MinimalStructType {
                struct_flags,
                header: MinimalStructHeader {
                    base_type,
                    detail: MinimalTypeDetail {},
                },
                member_seq: t
                    .members
                    .iter()
                    .zip(commons)
                    .map(|(m, common)| MinimalStructMember {
                        common,
                        detail: MinimalMemberDetail {
                            name_hash: name_hash(&m.name),
                        },
                    })
                    .collect(),
            }))
        })
    }

    fn union(&mut self, t: &UnionType) -> Result<TypeObject> {
        let union_flags = aggregate_flags(t.extensibility, t.nested, t.autoid_hash);
        // The discriminator is always must-understand: a reader cannot
        // read a union without it.
        let mut member_flags =
            t.discriminator_try_construct.flags() | MemberFlag::IS_MUST_UNDERSTAND;
        if t.discriminator_key {
            member_flags |= MemberFlag::IS_KEY;
        }
        let discriminator = CommonDiscriminatorMember {
            member_flags,
            type_id: self.identifier(&t.discriminator)?,
        };
        let mut commons = Vec::with_capacity(t.members.len());
        for m in &t.members {
            let mut member_flags = m.try_construct.flags();
            if m.external {
                member_flags |= MemberFlag::IS_EXTERNAL;
            }
            if m.default {
                member_flags |= MemberFlag::IS_DEFAULT;
            }
            let mut label_seq = m.labels.clone();
            label_seq.sort_unstable();
            commons.push(CommonUnionMember {
                member_id: m.id,
                member_flags,
                type_id: self.identifier(&m.ty)?,
                label_seq,
            });
        }
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Union(CompleteUnionType {
                union_flags,
                header: CompleteUnionHeader {
                    detail: type_detail(&t.name),
                },
                discriminator: CompleteDiscriminatorMember {
                    common: discriminator,
                    ann_builtin: None,
                    ann_custom: None,
                },
                member_seq: t
                    .members
                    .iter()
                    .zip(commons)
                    .map(|(m, common)| CompleteUnionMember {
                        common,
                        detail: member_detail(&m.name, Some(&m.annotations)),
                    })
                    .collect(),
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Union(MinimalUnionType {
                union_flags,
                header: MinimalUnionHeader {
                    detail: MinimalTypeDetail {},
                },
                discriminator: MinimalDiscriminatorMember {
                    common: discriminator,
                },
                member_seq: t
                    .members
                    .iter()
                    .zip(commons)
                    .map(|(m, common)| MinimalUnionMember {
                        common,
                        detail: MinimalMemberDetail {
                            name_hash: name_hash(&m.name),
                        },
                    })
                    .collect(),
            }))
        })
    }
}

fn string_identifier(wide: bool, bound: u32) -> TypeIdentifier {
    match u8::try_from(bound) {
        Ok(bound) => TypeIdentifier::StringSmall {
            wide,
            defn: StringSTypeDefn { bound },
        },
        Err(_) => TypeIdentifier::StringLarge {
            wide,
            defn: StringLTypeDefn { bound },
        },
    }
}

fn extensibility_flags(kind: Extensibility) -> TypeFlag {
    match kind {
        Extensibility::Final => TypeFlag::IS_FINAL,
        Extensibility::Appendable => TypeFlag::IS_APPENDABLE,
        Extensibility::Mutable => TypeFlag::IS_MUTABLE,
    }
}

fn aggregate_flags(kind: Extensibility, nested: bool, autoid_hash: bool) -> TypeFlag {
    let mut flags = extensibility_flags(kind);
    if nested {
        flags |= TypeFlag::IS_NESTED;
    }
    if autoid_hash {
        flags |= TypeFlag::IS_AUTOID_HASH;
    }
    flags
}

fn type_detail(name: &str) -> CompleteTypeDetail {
    CompleteTypeDetail {
        ann_builtin: None,
        ann_custom: None,
        type_name: name.to_owned(),
    }
}

fn member_detail(
    name: &str,
    annotations: Option<&AppliedBuiltinMemberAnnotations>,
) -> CompleteMemberDetail {
    CompleteMemberDetail {
        name: name.to_owned(),
        ann_builtin: annotations
            .filter(|a| **a != AppliedBuiltinMemberAnnotations::default())
            .cloned(),
        ann_custom: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_sequences_switch_to_large_at_256() {
        let ids = |ty: &Type| Builder::new(EK_MINIMAL).identifier(ty).unwrap();
        assert!(matches!(
            ids(&Type::String {
                wide: false,
                bound: 255
            }),
            TypeIdentifier::StringSmall { .. }
        ));
        assert!(matches!(
            ids(&Type::String {
                wide: true,
                bound: 256
            }),
            TypeIdentifier::StringLarge { wide: true, .. }
        ));
        let seq = |bound| {
            Type::Sequence(Arc::new(SequenceType {
                element: Element::new(Type::Primitive(Primitive::Int32)),
                bound,
            }))
        };
        assert!(matches!(ids(&seq(255)), TypeIdentifier::SeqSmall(_)));
        assert!(matches!(ids(&seq(256)), TypeIdentifier::SeqLarge(_)));
    }

    #[test]
    fn collections_of_named_types_carry_the_equivalence_kind() {
        let inner: Type = StructType {
            name: "M::Inner".into(),
            ..StructType::default()
        }
        .into();
        let seq = Type::sequence(inner);
        for kind in [EK_MINIMAL, EK_COMPLETE] {
            let id = Builder::new(kind).identifier(&seq).unwrap();
            assert_eq!(id.plain_collection_header().unwrap().equiv_kind, kind);
            assert!(!id.is_fully_descriptive());
        }
        let id = Builder::new(EK_MINIMAL)
            .identifier(&Type::sequence(Type::string()))
            .unwrap();
        assert_eq!(id.plain_collection_header().unwrap().equiv_kind, EK_BOTH);
    }

    #[test]
    fn a_type_used_twice_is_listed_once() {
        let inner: Type = StructType {
            name: "M::Inner".into(),
            ..StructType::default()
        }
        .into();
        let outer: Type = StructType {
            name: "M::Outer".into(),
            members: vec![
                StructMember::new("a", 0, inner.clone()),
                StructMember::new("b", 1, Type::sequence(inner)),
            ],
            ..StructType::default()
        }
        .into();
        let objects = TypeObjects::new(&outer).unwrap();
        assert_eq!(objects.minimal.len(), 2);
        assert_eq!(objects.information.complete.dependent_typeid_count, 1);
        for pair in objects.minimal.iter().chain(&objects.complete) {
            assert_eq!(objects.get(&pair.type_identifier), Some(&pair.type_object));
        }
        assert!(matches!(objects.minimal_id(), TypeIdentifier::Minimal(_)));
        assert!(matches!(objects.complete_id(), TypeIdentifier::Complete(_)));
    }

    #[test]
    fn only_named_types_have_type_objects() {
        assert!(matches!(
            TypeObjects::new(&Type::string()),
            Err(Error::InvalidType(_))
        ));
    }

    #[test]
    fn the_same_type_described_twice_has_the_same_identifiers() {
        let make = || -> Type {
            StructType {
                name: "M::S".into(),
                members: vec![StructMember::new(
                    "x",
                    0,
                    Type::Primitive(Primitive::Float64),
                )],
                ..StructType::default()
            }
            .into()
        };
        let a = TypeObjects::new(&make()).unwrap();
        let b = TypeObjects::new(&make()).unwrap();
        assert_eq!(a.information, b.information);
    }
}
