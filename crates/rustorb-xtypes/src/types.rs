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
//! 7.3.4.8) are made with [`Type::forward`] and [`Type::define`]; their
//! TypeObjects are identified by their strongly connected component
//! (7.3.4.9.2). A cycle must go through a sequence, a map or an optional
//! member, or the type would have no end.
//!
//! Bitsets ([`BitsetType`]) have TypeObjects (7.3.4.6) but no
//! serialization in XTypes 1.3: rustorb writes one as the smallest of
//! `uint8`, `uint16`, `uint32` and `uint64` that holds its bits, the first
//! field in the lowest bits (Fast DDS 2.11 writes each field as its own
//! holder instead, so the two do not agree). Annotation types ([`AnnotationType`]) have TypeObjects and
//! no samples.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

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

    /// The primitive type of a type kind, if it is one.
    pub fn of_kind(kind: TypeKind) -> Option<Primitive> {
        const ALL: [Primitive; 15] = [
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
        ALL.into_iter().find(|p| p.kind() == kind)
    }
}

/// A type.
///
/// Named types are behind an [`Arc`] so that a type used in several
/// places is described once.
///
/// Equality is structural, except that a forward declaration is equal to
/// any type of its name: that is where comparing recursive types stops.
#[derive(Debug, Clone)]
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
    /// A `bitset`.
    Bitset(Arc<BitsetType>),
    /// An annotation type (`@annotation`), whose TypeObject describes it
    /// (7.3.4.3); it is not the type of data.
    Annotation(Arc<AnnotationType>),
    /// A named type declared before it is defined, for types that depend
    /// on each other (7.3.4.8): it stands for the type it is defined as
    /// ([`Type::define`]).
    Forward(Arc<ForwardType>),
}

/// A forward declaration (`struct TreeNode;`): the name, and the type
/// once it is defined.
///
/// A recursive type made with it holds itself: it is never freed, which
/// is fine for types, made once.
pub struct ForwardType {
    name: String,
    target: OnceLock<Type>,
}

impl ForwardType {
    /// The qualified name it was declared with.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type, once defined.
    pub fn target(&self) -> Option<&Type> {
        self.target.get()
    }
}

impl fmt::Debug for ForwardType {
    // Not the target, which may contain this.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardType")
            .field("name", &self.name)
            .field("defined", &self.target.get().is_some())
            .finish()
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Forward(f), t) | (t, Type::Forward(f)) => t.name() == Some(f.name()),
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (
                Type::String { wide, bound },
                Type::String {
                    wide: w2,
                    bound: b2,
                },
            ) => wide == w2 && bound == b2,
            (Type::Sequence(a), Type::Sequence(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => a == b,
            (Type::Map(a), Type::Map(b)) => a == b,
            (Type::Alias(a), Type::Alias(b)) => a == b,
            (Type::Enum(a), Type::Enum(b)) => a == b,
            (Type::Bitmask(a), Type::Bitmask(b)) => a == b,
            (Type::Struct(a), Type::Struct(b)) => a == b,
            (Type::Union(a), Type::Union(b)) => a == b,
            (Type::Bitset(a), Type::Bitset(b)) => a == b,
            (Type::Annotation(a), Type::Annotation(b)) => a == b,
            _ => false,
        }
    }
}

impl PartialEq for ForwardType {
    // By name: comparing the targets would not end.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Type {
    /// A forward declaration of the named type `name`, to use in the
    /// types that refer to it before [`Type::define`] ties the knot:
    ///
    /// ```
    /// use rustorb_xtypes::types::*;
    ///
    /// // struct TreeNode { long data; sequence<TreeNode> children; };
    /// let fwd = Type::forward("TreeNode");
    /// let node = Type::from(StructType {
    ///     name: "TreeNode".into(),
    ///     members: vec![
    ///         StructMember::new("data", 0, Type::Primitive(Primitive::Int32)),
    ///         StructMember::new("children", 1, Type::sequence(fwd.clone())),
    ///     ],
    ///     ..StructType::default()
    /// });
    /// fwd.define(node.clone()).unwrap();
    /// assert!(TypeObjects::new(&node).is_ok());
    /// ```
    pub fn forward(name: impl Into<String>) -> Type {
        Type::Forward(Arc::new(ForwardType {
            name: name.into(),
            target: OnceLock::new(),
        }))
    }

    /// Defines a forward declaration as `target`, a named type of the
    /// same name. Fails for other types, a different name, or a forward
    /// declaration defined already.
    pub fn define(&self, target: Type) -> Result<()> {
        let Type::Forward(f) = self else {
            return Err(Error::BadParameter("not a forward declaration".into()));
        };
        let target_name = target.resolve().name();
        if matches!(target, Type::Forward(_)) || target_name != Some(f.name.as_str()) {
            return Err(Error::BadParameter(format!(
                "{} cannot be defined as {}",
                f.name,
                target_name.unwrap_or("an anonymous type")
            )));
        }
        f.target
            .set(target)
            .map_err(|_| Error::BadParameter(format!("{} is defined already", f.name)))
    }

    /// The type itself, or for a forward declaration the type it is
    /// defined as (itself while undefined).
    pub fn resolve(&self) -> &Type {
        match self {
            Type::Forward(f) => f.target.get().unwrap_or(self),
            _ => self,
        }
    }

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
            Type::Forward(f) => Some(&f.name),
            Type::Alias(t) => Some(&t.name),
            Type::Enum(t) => Some(&t.name),
            Type::Bitmask(t) => Some(&t.name),
            Type::Bitset(t) => Some(&t.name),
            Type::Annotation(t) => Some(&t.name),
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

impl From<BitsetType> for Type {
    fn from(t: BitsetType) -> Type {
        Type::Bitset(Arc::new(t))
    }
}

impl From<AnnotationType> for Type {
    fn from(t: AnnotationType) -> Type {
        Type::Annotation(Arc::new(t))
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

/// A field of a [`BitsetType`]: `bitfield<bitcount, holder> name;`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bitfield {
    /// The name.
    pub name: String,
    /// The first bit, counted from the least significant.
    pub position: u16,
    /// The number of bits, 1 to 64.
    pub bitcount: u8,
    /// The type its value is read and written as: boolean or an integer
    /// type.
    pub holder: Primitive,
}

/// A `bitset` (IDL 4.2, 7.4.13.4.3.2): fields of bits in an integer.
///
/// DDS-XTypes 1.3 has its TypeObject but no serialization of its own: it
/// is serialized here as the smallest unsigned integer of 8, 16, 32 or
/// 64 bits that holds every field, as a bitmask is (7.4.3.4.4 for
/// bitmasks).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BitsetType {
    /// The qualified name.
    pub name: String,
    /// The fields; unnamed ones (padding) are left out.
    pub fields: Vec<Bitfield>,
}

impl BitsetType {
    /// The number of bits that hold the fields: the end of the last one.
    pub fn bits(&self) -> u32 {
        self.fields
            .iter()
            .map(|f| u32::from(f.position) + u32::from(f.bitcount))
            .max()
            .unwrap_or(0)
    }

    /// The unsigned integer type it is serialized as.
    pub fn holder(&self) -> Primitive {
        match self.bits() {
            0..=8 => Primitive::Uint8,
            9..=16 => Primitive::Uint16,
            17..=32 => Primitive::Uint32,
            _ => Primitive::Uint64,
        }
    }

    /// Checks the fields: at most 64 bits, no field overlapping another,
    /// holder types that hold their bits.
    pub fn check(&self) -> Result<()> {
        let bad = |what: String| Err(Error::InvalidType(format!("bitset {}: {what}", self.name)));
        if self.bits() > 64 {
            return bad("more than 64 bits".into());
        }
        let mut used = 0u64;
        for f in &self.fields {
            if f.bitcount == 0 || f.bitcount > 64 {
                return bad(format!("{} has {} bits", f.name, f.bitcount));
            }
            let width = match f.holder {
                Primitive::Boolean => 1,
                Primitive::Byte | Primitive::Int8 | Primitive::Uint8 => 8,
                Primitive::Int16 | Primitive::Uint16 => 16,
                Primitive::Int32 | Primitive::Uint32 => 32,
                Primitive::Int64 | Primitive::Uint64 => 64,
                _ => return bad(format!("{} is not held by an integer type", f.name)),
            };
            if f.bitcount > width {
                return bad(format!("{} has more bits than its holder type", f.name));
            }
            let mask = (u64::MAX >> (64 - u32::from(f.bitcount))) << f.position;
            if used & mask != 0 {
                return bad(format!("{} overlaps another field", f.name));
            }
            used |= mask;
        }
        Ok(())
    }
}

/// A member of an [`AnnotationType`].
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationParameter {
    /// The name.
    pub name: String,
    /// The type: a primitive type, a string, an enum or an alias of one.
    pub ty: Type,
    /// The default value, if any.
    pub default: Option<AnnotationParameterValue>,
}

/// An annotation type: `@annotation name { members };` (7.2.2.4.6).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationType {
    /// The qualified name.
    pub name: String,
    /// The members.
    pub members: Vec<AnnotationParameter>,
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
    let size =
        |n: usize| u32::try_from(n).map_err(|_| Error::InvalidType("TypeObject too large".into()));
    let first = pairs
        .first()
        .ok_or_else(|| Error::InvalidType("no TypeObject".into()))?;
    let typeid_with_size = TypeIdentifierWithSize {
        type_id: first.type_identifier.clone(),
        typeobject_serialized_size: size(first.type_object.to_bytes()?.len())?,
    };
    // The types of a strongly connected component are listed as the
    // component (7.6.3.2.2), with the size of their TypeObjects in
    // sequence; the type's own component too when it has other types.
    let mut dependent_typeids: Vec<TypeIdentifierWithSize> = Vec::new();
    let own = first.type_identifier.scc_identifier();
    for p in pairs {
        let type_id = match p.type_identifier.scc_identifier() {
            Some(scc) => {
                let alone = p.type_identifier.scc_members().len() == 1;
                if Some(&scc) == own.as_ref() && alone {
                    continue;
                }
                scc
            }
            None if std::ptr::eq(p, first) => continue,
            None => p.type_identifier.clone(),
        };
        if dependent_typeids.iter().any(|d| d.type_id == type_id) {
            continue;
        }
        let bytes = if type_id.is_scc_identifier() {
            let mut members: Vec<&TypeIdentifierTypeObjectPair> = pairs
                .iter()
                .filter(|q| q.type_identifier.scc_identifier().as_ref() == Some(&type_id))
                .collect();
            members.sort_by_key(|q| q.type_identifier.clone());
            let objects: Vec<TypeObject> = members.iter().map(|q| q.type_object.clone()).collect();
            crate::xcdr2::to_bytes(&objects, rustorb_cdr::ByteOrder::Little)?
        } else {
            p.type_object.to_bytes()?
        };
        dependent_typeids.push(TypeIdentifierWithSize {
            type_id,
            typeobject_serialized_size: size(bytes.len())?,
        });
    }
    Ok(TypeIdentifierWithDependencies {
        typeid_with_size,
        dependent_typeid_count: i32::try_from(dependent_typeids.len())
            .map_err(|_| Error::InvalidType("too many types".into()))?,
        dependent_typeids,
    })
}

/// Computes TypeIdentifiers of one equivalence kind, collecting the
/// TypeObjects of the named types on the way.
///
/// The types are taken in the order of the strongly connected
/// components of their dependency graph (7.3.4.9.2), those depended on
/// first; its vertices are the named types and the anonymous
/// collections (sequences, arrays, maps). A named type alone in its
/// component, that does not use itself, is identified by the hash of its
/// TypeObject, and an anonymous collection outside a cycle by a plain
/// identifier. The types of a component that has a cycle, anonymous
/// collections included (as `sequence<TreeNode>` in 7.3.4.9.1), get
/// `TI_STRONGLY_CONNECTED_COMPONENT` identifiers from the hash of their
/// TypeObjects together.
struct Builder {
    kind: EquivalenceKind,
    /// The identifiers of the named types computed so far, by address.
    ids: HashMap<usize, TypeIdentifier>,
}

fn address(ty: &Type) -> Option<usize> {
    Some(match ty.resolve() {
        Type::Alias(t) => Arc::as_ptr(t) as usize,
        Type::Enum(t) => Arc::as_ptr(t) as usize,
        Type::Bitmask(t) => Arc::as_ptr(t) as usize,
        Type::Struct(t) => Arc::as_ptr(t) as usize,
        Type::Union(t) => Arc::as_ptr(t) as usize,
        Type::Bitset(t) => Arc::as_ptr(t) as usize,
        Type::Annotation(t) => Arc::as_ptr(t) as usize,
        _ => return None,
    })
}

/// The address of a type that is a vertex of the dependency graph: a
/// named type or an anonymous collection.
fn vertex(ty: &Type) -> Option<usize> {
    address(ty).or_else(|| match ty.resolve() {
        Type::Sequence(t) => Some(Arc::as_ptr(t) as usize),
        Type::Array(t) => Some(Arc::as_ptr(t) as usize),
        Type::Map(t) => Some(Arc::as_ptr(t) as usize),
        _ => None,
    })
}

/// The dependency graph of the types reachable from a type
/// (7.3.4.9.2): its named types and anonymous collections.
#[derive(Default)]
struct Graph {
    /// The types: named types and anonymous collections (sequences,
    /// arrays, maps), in the order they are first used (depth first).
    types: Vec<Type>,
    /// Their names for the order of a strongly connected component
    /// (7.3.4.9.2 step 4.b): the qualified name of a named type; for an
    /// anonymous type, that of the type that contains it, with `.` and
    /// the member's name for a member.
    names: Vec<String>,
    index: HashMap<usize, usize>,
    /// For each type, the types it uses directly, and whether the use
    /// can be empty (the element of a sequence or a map, an optional
    /// member), which keeps a cycle finite.
    edges: Vec<Vec<(usize, bool)>>,
}

impl Graph {
    fn of(ty: &Type) -> Result<Graph> {
        let mut g = Graph::default();
        g.visit(ty, "")?;
        Ok(g)
    }

    /// Adds `ty` and the types it uses; `name` is the name of an
    /// anonymous type. None for a type that is not a vertex (a primitive
    /// type or a string).
    fn visit(&mut self, ty: &Type, name: &str) -> Result<Option<usize>> {
        let ty = defined(ty)?;
        let Some(address) = vertex(ty) else {
            return Ok(None);
        };
        if let Some(&i) = self.index.get(&address) {
            return Ok(Some(i));
        }
        let i = self.types.len();
        let name = ty.name().unwrap_or(name).to_owned();
        self.types.push(ty.clone());
        self.names.push(name.clone());
        self.index.insert(address, i);
        self.edges.push(Vec::new());
        let member = |m: &str| format!("{name}.{m}");
        let mut uses: Vec<(&Type, bool, String)> = Vec::new();
        match ty {
            Type::Alias(t) => uses.push((&t.related, false, name.clone())),
            Type::Struct(t) => {
                if let Some(base) = &t.base {
                    uses.push((base, false, name.clone()));
                }
                for m in &t.members {
                    uses.push((&m.ty, m.optional, member(&m.name)));
                }
            }
            Type::Union(t) => {
                uses.push((&t.discriminator, false, name.clone()));
                for m in &t.members {
                    uses.push((&m.ty, false, member(&m.name)));
                }
            }
            Type::Annotation(t) => {
                for m in &t.members {
                    uses.push((&m.ty, false, member(&m.name)));
                }
            }
            Type::Sequence(t) => uses.push((&t.element.ty, true, name.clone())),
            Type::Array(t) => uses.push((&t.element.ty, false, name.clone())),
            Type::Map(t) => {
                uses.push((&t.key.ty, true, name.clone()));
                uses.push((&t.element.ty, true, name.clone()));
            }
            _ => {}
        }
        for (u, can_be_empty, name) in uses {
            if let Some(j) = self.visit(u, &name)? {
                self.edges[i].push((j, can_be_empty));
            }
        }
        Ok(Some(i))
    }

    /// The strongly connected components (Tarjan), each one after the
    /// components it depends on.
    fn components(&self) -> Vec<Vec<usize>> {
        struct Tarjan<'a> {
            g: &'a Graph,
            next: usize,
            index: Vec<Option<usize>>,
            low: Vec<usize>,
            stack: Vec<usize>,
            on_stack: Vec<bool>,
            out: Vec<Vec<usize>>,
        }
        impl Tarjan<'_> {
            fn connect(&mut self, v: usize) {
                self.index[v] = Some(self.next);
                self.low[v] = self.next;
                self.next += 1;
                self.stack.push(v);
                self.on_stack[v] = true;
                for &(w, _) in &self.g.edges[v] {
                    match self.index[w] {
                        None => {
                            self.connect(w);
                            self.low[v] = self.low[v].min(self.low[w]);
                        }
                        Some(iw) if self.on_stack[w] => self.low[v] = self.low[v].min(iw),
                        Some(_) => {}
                    }
                }
                if Some(self.low[v]) == self.index[v] {
                    let mut c = Vec::new();
                    loop {
                        let w = self.stack.pop().expect("on the stack");
                        self.on_stack[w] = false;
                        c.push(w);
                        if w == v {
                            break;
                        }
                    }
                    self.out.push(c);
                }
            }
        }
        let n = self.types.len();
        let mut t = Tarjan {
            g: self,
            next: 0,
            index: vec![None; n],
            low: vec![0; n],
            stack: Vec::new(),
            on_stack: vec![false; n],
            out: Vec::new(),
        };
        for v in 0..n {
            if t.index[v].is_none() {
                t.connect(v);
            }
        }
        t.out
    }

    /// Whether the types of `component` use each other in a cycle.
    fn cyclic(&self, component: &[usize]) -> bool {
        component.len() > 1
            || self.edges[component[0]]
                .iter()
                .any(|&(w, _)| w == component[0])
    }

    /// Refuses a cycle of the component that no value can end: one that
    /// goes through no sequence, map or optional member.
    fn check_finite(&self, component: &[usize]) -> Result<()> {
        // Depth-first search for a cycle among the edges that cannot be
        // empty, inside the component.
        fn has_cycle(
            g: &Graph,
            inside: &[usize],
            v: usize,
            state: &mut HashMap<usize, bool>,
        ) -> bool {
            state.insert(v, true);
            for &(w, can_be_empty) in &g.edges[v] {
                if can_be_empty || !inside.contains(&w) {
                    continue;
                }
                match state.get(&w) {
                    Some(true) => return true,
                    Some(false) => {}
                    None => {
                        if has_cycle(g, inside, w, state) {
                            return true;
                        }
                    }
                }
            }
            state.insert(v, false);
            false
        }
        let mut state = HashMap::new();
        for &v in component {
            if !state.contains_key(&v) && has_cycle(self, component, v, &mut state) {
                return Err(Error::InvalidType(format!(
                    "{} contains itself without a sequence, map or optional member \
                     in between: its values would never end",
                    self.names[v]
                )));
            }
        }
        Ok(())
    }
}

/// The value of a parameter of an annotation type that has no default:
/// zero, false, empty, or the default literal of an enum.
fn zero_value(ty: &Type) -> Result<AnnotationParameterValue> {
    use AnnotationParameterValue as V;
    let mut ty = defined(ty)?;
    while let Type::Alias(a) = ty {
        ty = defined(&a.related)?;
    }
    Ok(match ty {
        Type::Primitive(p) => match p {
            Primitive::Boolean => V::Boolean(false),
            Primitive::Byte => V::Byte(0),
            Primitive::Int8 => V::Int8(0),
            Primitive::Uint8 => V::Uint8(0),
            Primitive::Int16 => V::Int16(0),
            Primitive::Uint16 => V::Uint16(0),
            Primitive::Int32 => V::Int32(0),
            Primitive::Uint32 => V::Uint32(0),
            Primitive::Int64 => V::Int64(0),
            Primitive::Uint64 => V::Uint64(0),
            Primitive::Float32 => V::Float32(0.0),
            Primitive::Float64 => V::Float64(0.0),
            Primitive::Float128 => V::Float128(rustorb_cdr::LongDouble::ZERO),
            Primitive::Char8 => V::Char8(0),
            Primitive::Char16 => V::Char16(0),
        },
        Type::String { wide: false, .. } => V::String8(String::new()),
        Type::String { wide: true, .. } => V::String16(String::new()),
        Type::Enum(e) => V::Enumerated(
            e.literals
                .iter()
                .find(|l| l.default)
                .or_else(|| e.literals.first())
                .map_or(0, |l| l.value),
        ),
        other => {
            return Err(Error::InvalidType(format!(
                "an annotation parameter of type {}",
                other
                    .name()
                    .unwrap_or("other than a primitive, string or enum")
            )));
        }
    })
}

/// Whether the named type `ty` uses itself, directly or through other
/// types (it is in a cycle of the dependency graph, 7.3.4.8).
pub(crate) fn uses_itself(ty: &Type) -> bool {
    let Ok(g) = Graph::of(ty) else {
        return false;
    };
    g.components()
        .iter()
        .find(|c| c.contains(&0))
        .is_some_and(|c| g.cyclic(c))
}

/// `ty`, a forward declaration resolved; an error for one not defined.
fn defined(ty: &Type) -> Result<&Type> {
    match ty.resolve() {
        Type::Forward(f) => Err(Error::InvalidType(format!(
            "the forward declaration {} is not defined",
            f.name()
        ))),
        t => Ok(t),
    }
}

impl Builder {
    fn new(kind: EquivalenceKind) -> Builder {
        Builder {
            kind,
            ids: HashMap::new(),
        }
    }

    fn run(mut self, ty: &Type) -> Result<Vec<TypeIdentifierTypeObjectPair>> {
        if address(ty).is_none() {
            return Err(Error::InvalidType(
                "only a named type has a TypeObject".into(),
            ));
        }
        let g = Graph::of(ty)?;
        let mut objects: Vec<Option<TypeIdentifierTypeObjectPair>> = vec![None; g.types.len()];
        for component in g.components() {
            if !g.cyclic(&component) {
                let v = component[0];
                if address(&g.types[v]).is_none() {
                    // An anonymous collection outside a cycle: a plain
                    // identifier, made where it is used.
                    continue;
                }
                let type_object = self.type_object(&g.types[v])?;
                let type_identifier = type_object.type_identifier()?;
                self.ids.insert(
                    address(&g.types[v]).expect("named"),
                    type_identifier.clone(),
                );
                objects[v] = Some(TypeIdentifierTypeObjectPair {
                    type_identifier,
                    type_object,
                });
                continue;
            }
            g.check_finite(&component)?;
            for (i, pair) in self.component(&g, component)? {
                objects[i] = Some(pair);
            }
        }
        Ok(objects.into_iter().flatten().collect())
    }

    /// The TypeObjects of the types of a strongly connected component
    /// with a cycle (7.3.4.9.2 step 4.b to 4.f).
    fn component(
        &mut self,
        g: &Graph,
        mut component: Vec<usize>,
    ) -> Result<Vec<(usize, TypeIdentifierTypeObjectPair)>> {
        // Sorted by qualified name: the SCCIndex, from 1.
        component.sort_by(|&a, &b| g.names[a].cmp(&g.names[b]).then(a.cmp(&b)));
        let scc_length = i32::try_from(component.len())
            .map_err(|_| Error::InvalidType("too many types".into()))?;
        let kind = self.kind;
        let set_ids = |ids: &mut HashMap<usize, TypeIdentifier>, hash: EquivalenceHash| {
            for (n, &v) in component.iter().enumerate() {
                let sc_component_id = if kind == EK_COMPLETE {
                    TypeObjectHashId::Complete(hash)
                } else {
                    TypeObjectHashId::Minimal(hash)
                };
                ids.insert(
                    vertex(&g.types[v]).expect("a vertex"),
                    TypeIdentifier::StronglyConnectedComponent(StronglyConnectedComponentId {
                        sc_component_id,
                        scc_length,
                        scc_index: n as i32 + 1,
                    }),
                );
            }
        };
        // The TypeObjects with the temporary identifiers, of hash 0,
        // serialized in sequence (XCDR2, little endian) and hashed.
        set_ids(&mut self.ids, [0; 14]);
        let temporary = component
            .iter()
            .map(|&v| self.type_object(&g.types[v]))
            .collect::<Result<TypeObjectSeq>>()?;
        let hash = hash_of(&crate::xcdr2::to_bytes(
            &temporary,
            rustorb_cdr::ByteOrder::Little,
        )?);
        set_ids(&mut self.ids, hash);
        // The TypeObjects refer to the others of the component by their
        // final identifiers.
        component
            .iter()
            .map(|&v| {
                let ty = &g.types[v];
                Ok((
                    v,
                    TypeIdentifierTypeObjectPair {
                        type_identifier: self.ids[&vertex(ty).expect("a vertex")].clone(),
                        type_object: self.type_object(ty)?,
                    },
                ))
            })
            .collect()
    }

    fn identifier(&mut self, ty: &Type) -> Result<TypeIdentifier> {
        // An anonymous collection of a strongly connected component.
        if let Some(id) = vertex(ty).and_then(|v| self.ids.get(&v)) {
            return Ok(id.clone());
        }
        Ok(match ty.resolve() {
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
                self.ids.get(&address).cloned().ok_or_else(|| {
                    Error::InvalidType(format!(
                        "{} is used before it is built",
                        named.name().unwrap_or_default()
                    ))
                })?
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
            Type::Bitset(t) => self.bitset(t),
            Type::Annotation(t) => self.annotation(t),
            Type::Sequence(_) | Type::Array(_) | Type::Map(_) => self.collection(ty),
            _ => unreachable!("only named types and collections have TypeObjects"),
        }
    }

    /// The TypeObject of an anonymous collection, which only one of a
    /// strongly connected component has (7.3.4.9.1).
    fn collection(&mut self, ty: &Type) -> Result<TypeObject> {
        let complete = self.complete();
        let mut element =
            |e: &Element| -> Result<(CompleteCollectionElement, MinimalCollectionElement)> {
                let common = CommonCollectionElement {
                    element_flags: e.flags(),
                    r#type: self.identifier(&e.ty)?,
                };
                Ok((
                    CompleteCollectionElement {
                        common: common.clone(),
                        detail: CompleteElementDetail::default(),
                    },
                    MinimalCollectionElement { common },
                ))
            };
        let collection_flag = CollectionTypeFlag::default();
        Ok(match ty {
            Type::Sequence(t) => {
                let (c, m) = element(&t.element)?;
                let common = CommonCollectionHeader { bound: t.bound };
                if complete {
                    TypeObject::Complete(CompleteTypeObject::Sequence(CompleteSequenceType {
                        collection_flag,
                        header: CompleteCollectionHeader {
                            common,
                            detail: None,
                        },
                        element: c,
                    }))
                } else {
                    TypeObject::Minimal(MinimalTypeObject::Sequence(MinimalSequenceType {
                        collection_flag,
                        header: MinimalCollectionHeader { common },
                        element: m,
                    }))
                }
            }
            Type::Array(t) => {
                let (c, m) = element(&t.element)?;
                let common = CommonArrayHeader {
                    bound_seq: t.dimensions.clone(),
                };
                if complete {
                    TypeObject::Complete(CompleteTypeObject::Array(CompleteArrayType {
                        collection_flag,
                        header: CompleteArrayHeader {
                            common,
                            detail: type_detail(""),
                        },
                        element: c,
                    }))
                } else {
                    TypeObject::Minimal(MinimalTypeObject::Array(MinimalArrayType {
                        collection_flag,
                        header: MinimalArrayHeader { common },
                        element: m,
                    }))
                }
            }
            Type::Map(t) => {
                let (ck, mk) = element(&t.key)?;
                let (ce, me) = element(&t.element)?;
                let common = CommonCollectionHeader { bound: t.bound };
                if complete {
                    TypeObject::Complete(CompleteTypeObject::Map(CompleteMapType {
                        collection_flag,
                        header: CompleteCollectionHeader {
                            common,
                            detail: None,
                        },
                        key: ck,
                        element: ce,
                    }))
                } else {
                    TypeObject::Minimal(MinimalTypeObject::Map(MinimalMapType {
                        collection_flag,
                        header: MinimalCollectionHeader { common },
                        key: mk,
                        element: me,
                    }))
                }
            }
            _ => unreachable!("a collection"),
        })
    }

    fn bitset(&mut self, t: &BitsetType) -> Result<TypeObject> {
        t.check()?;
        let mut fields: Vec<&Bitfield> = t.fields.iter().collect();
        fields.sort_by_key(|f| f.position);
        let common = |f: &Bitfield| CommonBitfield {
            position: f.position,
            flags: MemberFlag::default(),
            bitcount: f.bitcount,
            holder_type: f.holder.kind(),
        };
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Bitset(CompleteBitsetType {
                bitset_flags: TypeFlag::default(),
                header: CompleteBitsetHeader {
                    detail: type_detail(&t.name),
                },
                field_seq: fields
                    .into_iter()
                    .map(|f| CompleteBitfield {
                        common: common(f),
                        detail: member_detail(&f.name, None),
                    })
                    .collect(),
            }))
        } else {
            TypeObject::Minimal(MinimalTypeObject::Bitset(MinimalBitsetType {
                bitset_flags: TypeFlag::default(),
                header: MinimalBitsetHeader {},
                field_seq: fields
                    .into_iter()
                    .map(|f| MinimalBitfield {
                        common: common(f),
                        name_hash: name_hash(&f.name),
                    })
                    .collect(),
            }))
        })
    }

    fn annotation(&mut self, t: &AnnotationType) -> Result<TypeObject> {
        let mut commons = Vec::with_capacity(t.members.len());
        for m in &t.members {
            let default_value = match &m.default {
                Some(v) => v.clone(),
                None => zero_value(&m.ty)?,
            };
            commons.push((
                CommonAnnotationParameter {
                    member_flags: MemberFlag::default(),
                    member_type_id: self.identifier(&m.ty)?,
                },
                default_value,
            ));
        }
        Ok(if self.complete() {
            TypeObject::Complete(CompleteTypeObject::Annotation(CompleteAnnotationType {
                annotation_flag: TypeFlag::default(),
                header: CompleteAnnotationHeader {
                    annotation_name: t.name.clone(),
                },
                member_seq: t
                    .members
                    .iter()
                    .zip(commons)
                    .map(|(m, (common, default_value))| CompleteAnnotationParameter {
                        common,
                        name: m.name.clone(),
                        default_value,
                    })
                    .collect(),
            }))
        } else {
            // Ordered by the hash of the names (7.3.4.5).
            let mut member_seq: Vec<MinimalAnnotationParameter> = t
                .members
                .iter()
                .zip(commons)
                .map(|(m, (common, default_value))| MinimalAnnotationParameter {
                    common,
                    name_hash: name_hash(&m.name),
                    default_value,
                })
                .collect();
            member_seq.sort_by_key(|m| m.name_hash);
            TypeObject::Minimal(MinimalTypeObject::Annotation(MinimalAnnotationType {
                annotation_flag: TypeFlag::default(),
                header: MinimalAnnotationHeader {},
                member_seq,
            }))
        })
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
        let seq = Type::sequence(inner.clone());
        for kind in [EK_MINIMAL, EK_COMPLETE] {
            let mut b = Builder::new(kind);
            // As if built.
            b.ids
                .insert(address(&inner).unwrap(), TypeIdentifier::Minimal([1; 14]));
            let id = b.identifier(&seq).unwrap();
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

    /// struct TreeNode { long data; sequence<TreeNode> children; }.
    fn tree() -> Type {
        let fwd = Type::forward("M::TreeNode");
        let node = Type::from(StructType {
            name: "M::TreeNode".into(),
            members: vec![
                StructMember::new("data", 0, Type::Primitive(Primitive::Int32)),
                StructMember::new("children", 1, Type::sequence(fwd.clone())),
            ],
            ..StructType::default()
        });
        fwd.define(node.clone()).unwrap();
        node
    }

    fn scc(id: &TypeIdentifier) -> StronglyConnectedComponentId {
        match id {
            TypeIdentifier::StronglyConnectedComponent(c) => *c,
            other => panic!("not an SCC identifier: {other:?}"),
        }
    }

    /// Checks the hash of a component the way 7.3.4.9.2 makes it: the
    /// TypeObjects with the hash zeroed in their identifiers, in sequence.
    fn check_hash(pairs: &[&TypeIdentifierTypeObjectPair]) {
        let c = scc(&pairs[0].type_identifier);
        let hash = match c.sc_component_id {
            TypeObjectHashId::Complete(h) | TypeObjectHashId::Minimal(h) => h,
        };
        let mut seq = Vec::new();
        for p in pairs {
            let mut bytes = p.type_object.to_bytes().unwrap();
            // The 14 octets of the hash appear only as the hash.
            let mut i = 0;
            while i + 14 <= bytes.len() {
                if bytes[i..i + 14] == hash {
                    bytes[i..i + 14].fill(0);
                    i += 14;
                } else {
                    i += 1;
                }
            }
            seq.push(TypeObject::from_bytes(&bytes).unwrap());
        }
        let bytes = crate::xcdr2::to_bytes(&seq, rustorb_cdr::ByteOrder::Little).unwrap();
        assert_eq!(hash_of(&bytes), hash);
    }

    #[test]
    fn a_type_that_contains_itself() {
        let objects = TypeObjects::new(&tree()).unwrap();
        for (pairs, complete) in [(&objects.minimal, false), (&objects.complete, true)] {
            // The struct and its anonymous sequence<M::TreeNode>
            // (7.3.4.9.1), sorted by name: "M::TreeNode" is 1,
            // "M::TreeNode.children" 2.
            assert_eq!(pairs.len(), 2);
            let (node, seq) = (
                scc(&pairs[0].type_identifier),
                scc(&pairs[1].type_identifier),
            );
            assert_eq!((node.scc_length, node.scc_index), (2, 1));
            assert_eq!((seq.scc_length, seq.scc_index), (2, 2));
            assert_eq!(node.sc_component_id, seq.sc_component_id);
            assert_eq!(
                matches!(node.sc_component_id, TypeObjectHashId::Complete(_)),
                complete
            );
            assert!(matches!(
                pairs[1].type_object,
                TypeObject::Complete(CompleteTypeObject::Sequence(_))
                    | TypeObject::Minimal(MinimalTypeObject::Sequence(_))
            ));
            // The struct uses the sequence, whose element is the struct.
            assert!(
                pairs[0]
                    .type_object
                    .referenced_types()
                    .contains(&&pairs[1].type_identifier)
            );
            assert_eq!(
                pairs[1].type_object.referenced_types(),
                vec![&pairs[0].type_identifier]
            );
            check_hash(&[&pairs[0], &pairs[1]]);
        }
        assert_eq!(
            objects.information.minimal.typeid_with_size.type_id,
            objects.minimal[0].type_identifier
        );
        // The same each time.
        assert_eq!(TypeObjects::new(&tree()).unwrap(), objects);
    }

    #[test]
    fn types_that_contain_each_other() {
        // struct Root { M::B b; };
        // struct B { sequence<A> a; };   struct A { @optional B b; long x; };
        let fwd_a = Type::forward("M::A");
        let b = Type::from(StructType {
            name: "M::B".into(),
            members: vec![StructMember::new("a", 0, Type::sequence(fwd_a.clone()))],
            ..StructType::default()
        });
        let a = Type::from(StructType {
            name: "M::A".into(),
            members: vec![
                StructMember {
                    optional: true,
                    ..StructMember::new("b", 0, b.clone())
                },
                StructMember::new("x", 1, Type::Primitive(Primitive::Int32)),
            ],
            ..StructType::default()
        });
        fwd_a.define(a).unwrap();
        let root = Type::from(StructType {
            name: "M::Root".into(),
            members: vec![StructMember::new("b", 0, b)],
            ..StructType::default()
        });
        let objects = TypeObjects::new(&root).unwrap();
        for pairs in [&objects.minimal, &objects.complete] {
            // Root, then B, B's sequence<A> and A in the order they are
            // first used.
            assert_eq!(pairs.len(), 4);
            assert!(matches!(
                pairs[0].type_identifier,
                TypeIdentifier::Minimal(_) | TypeIdentifier::Complete(_)
            ));
            let (b, seq, a) = (
                scc(&pairs[1].type_identifier),
                scc(&pairs[2].type_identifier),
                scc(&pairs[3].type_identifier),
            );
            // Sorted by name: "M::A" is 1, "M::B" 2, "M::B.a" 3.
            assert_eq!((a.scc_length, a.scc_index), (3, 1));
            assert_eq!((b.scc_length, b.scc_index), (3, 2));
            assert_eq!((seq.scc_length, seq.scc_index), (3, 3));
            assert_eq!(a.sc_component_id, b.sc_component_id);
            assert_eq!(a.sc_component_id, seq.sc_component_id);
            check_hash(&[&pairs[3], &pairs[1], &pairs[2]]);
            assert_eq!(
                pairs[0].type_object.referenced_types(),
                vec![&pairs[1].type_identifier]
            );
        }
        // B and A are listed as their component.
        let deps = &objects.information.complete.dependent_typeids;
        assert_eq!(deps.len(), 1);
        assert_eq!(
            Some(&deps[0].type_id),
            objects.complete[1]
                .type_identifier
                .scc_identifier()
                .as_ref()
        );
        let mut registry = crate::registry::TypeRegistry::new();
        registry.add_type(&objects);
        assert_eq!(
            registry.with_size(&deps[0].type_id).as_ref(),
            Some(&deps[0])
        );
        let root = &objects.information.complete.typeid_with_size.type_id;
        assert_eq!(
            registry.dependencies(root).unwrap(),
            vec![deps[0].type_id.clone()]
        );
        // What another participant does with them: checked together.
        let mut other = crate::registry::TypeRegistry::new();
        assert_eq!(other.add_pairs(&objects.complete[1..]).unwrap(), 3);
        assert!(other.knows(&deps[0].type_id));
        // One alone is not added, nor a component whose hash is wrong.
        let mut other = crate::registry::TypeRegistry::new();
        assert_eq!(other.add_pairs(&objects.complete[1..2]).unwrap(), 0);
        let mut wrong = objects.complete[1..].to_vec();
        wrong.swap(0, 1);
        let ids: Vec<_> = wrong.iter().map(|p| p.type_identifier.clone()).collect();
        wrong[0].type_identifier = ids[1].clone();
        wrong[1].type_identifier = ids[0].clone();
        assert_eq!(other.add_pairs(&wrong).unwrap(), 0);
    }

    #[test]
    fn endless_and_undefined_types_are_refused() {
        // struct S { S s; }: no value ends.
        let fwd = Type::forward("S");
        let s = Type::from(StructType {
            name: "S".into(),
            members: vec![StructMember::new("s", 0, fwd.clone())],
            ..StructType::default()
        });
        fwd.define(s.clone()).unwrap();
        assert!(matches!(TypeObjects::new(&s), Err(Error::InvalidType(_))));
        // A forward declaration never defined.
        let t = Type::from(StructType {
            name: "T".into(),
            members: vec![StructMember::new("u", 0, Type::forward("U"))],
            ..StructType::default()
        });
        assert!(matches!(TypeObjects::new(&t), Err(Error::InvalidType(_))));
        // Defined as another name, or twice.
        let fwd = Type::forward("V");
        assert!(fwd.define(t.clone()).is_err());
        let v = Type::from(StructType {
            name: "V".into(),
            ..StructType::default()
        });
        fwd.define(v.clone()).unwrap();
        assert!(fwd.define(v.clone()).is_err());
        assert_eq!(fwd, v);
    }
}
