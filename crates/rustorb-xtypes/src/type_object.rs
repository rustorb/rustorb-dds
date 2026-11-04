//! TypeIdentifier, TypeObject and TypeInformation: the types of DDS-XTypes
//! 1.3 Annex B and clause 7.6.3.2.1, with their XCDR2 serialization
//! ([`Xcdr2`]) and the hashes that identify types (7.3.4).
//!
//! The Rust types follow the IDL of Annex B: a struct per IDL struct, with
//! the same member names, and an enum per IDL union. The constants of the
//! IDL (`TK_*`, `TI_*`, `EK_*`) are in this module with their IDL names.
//! To build these objects from a description of a type, see
//! [`crate::types`].
#![allow(missing_docs)] // The members are those of Annex B, by name.

use rustorb_cdr::{ByteOrder, Decoder, Encoder, LongDouble};

use crate::xcdr2::{self, Xcdr2, encode_empty_mutable, skip_mutable, xstruct};
use crate::{Error, Result};

// ---------- Equivalence kinds -------------------------------------------

/// `EquivalenceKind`.
pub type EquivalenceKind = u8;
pub const EK_MINIMAL: EquivalenceKind = 0xF1;
pub const EK_COMPLETE: EquivalenceKind = 0xF2;
pub const EK_BOTH: EquivalenceKind = 0xF3;

// ---------- Type kinds ---------------------------------------------------

/// `TypeKind`.
pub type TypeKind = u8;
pub const TK_NONE: TypeKind = 0x00;
pub const TK_BOOLEAN: TypeKind = 0x01;
pub const TK_BYTE: TypeKind = 0x02;
pub const TK_INT16: TypeKind = 0x03;
pub const TK_INT32: TypeKind = 0x04;
pub const TK_INT64: TypeKind = 0x05;
pub const TK_UINT16: TypeKind = 0x06;
pub const TK_UINT32: TypeKind = 0x07;
pub const TK_UINT64: TypeKind = 0x08;
pub const TK_FLOAT32: TypeKind = 0x09;
pub const TK_FLOAT64: TypeKind = 0x0A;
pub const TK_FLOAT128: TypeKind = 0x0B;
pub const TK_INT8: TypeKind = 0x0C;
pub const TK_UINT8: TypeKind = 0x0D;
pub const TK_CHAR8: TypeKind = 0x10;
pub const TK_CHAR16: TypeKind = 0x11;
pub const TK_STRING8: TypeKind = 0x20;
pub const TK_STRING16: TypeKind = 0x21;
pub const TK_ALIAS: TypeKind = 0x30;
pub const TK_ENUM: TypeKind = 0x40;
pub const TK_BITMASK: TypeKind = 0x41;
pub const TK_ANNOTATION: TypeKind = 0x50;
pub const TK_STRUCTURE: TypeKind = 0x51;
pub const TK_UNION: TypeKind = 0x52;
pub const TK_BITSET: TypeKind = 0x53;
pub const TK_SEQUENCE: TypeKind = 0x60;
pub const TK_ARRAY: TypeKind = 0x61;
pub const TK_MAP: TypeKind = 0x62;

// ---------- Extra type identifier kinds ---------------------------------

pub const TI_STRING8_SMALL: u8 = 0x70;
pub const TI_STRING8_LARGE: u8 = 0x71;
pub const TI_STRING16_SMALL: u8 = 0x72;
pub const TI_STRING16_LARGE: u8 = 0x73;
pub const TI_PLAIN_SEQUENCE_SMALL: u8 = 0x80;
pub const TI_PLAIN_SEQUENCE_LARGE: u8 = 0x81;
pub const TI_PLAIN_ARRAY_SMALL: u8 = 0x90;
pub const TI_PLAIN_ARRAY_LARGE: u8 = 0x91;
pub const TI_PLAIN_MAP_SMALL: u8 = 0xA0;
pub const TI_PLAIN_MAP_LARGE: u8 = 0xA1;
pub const TI_STRONGLY_CONNECTED_COMPONENT: u8 = 0xB0;

/// Whether `kind` is a primitive type: one whose TypeIdentifier is the
/// kind alone (Table 29).
pub fn is_primitive_kind(kind: TypeKind) -> bool {
    matches!(kind, TK_BOOLEAN..=TK_UINT8 | TK_CHAR8 | TK_CHAR16)
}

pub const MEMBER_NAME_MAX_LENGTH: usize = 256;
pub const TYPE_NAME_MAX_LENGTH: usize = 256;

/// `MemberName`, at most [`MEMBER_NAME_MAX_LENGTH`] characters.
pub type MemberName = String;
/// `QualifiedTypeName`: module names and the type name separated by `::`,
/// with no leading `::`.
pub type QualifiedTypeName = String;
/// `PrimitiveTypeId`.
pub type PrimitiveTypeId = u8;
/// The first 14 octets of the MD5 of a serialized TypeObject.
pub type EquivalenceHash = [u8; 14];
/// The first 4 octets of the MD5 of a member name in UTF-8.
pub type NameHash = [u8; 4];
/// `LBound`.
pub type LBound = u32;
pub type LBoundSeq = Vec<LBound>;
pub const INVALID_LBOUND: LBound = 0;
/// `SBound`.
pub type SBound = u8;
pub type SBoundSeq = Vec<SBound>;
pub const INVALID_SBOUND: SBound = 0;
/// `MemberId`.
pub type MemberId = u32;

pub const ANNOTATION_STR_VALUE_MAX_LEN: usize = 128;
pub const ANNOTATION_OCTETSEC_VALUE_MAX_LEN: usize = 128;

/// The [`NameHash`] of `name`.
pub fn name_hash(name: &str) -> NameHash {
    let digest = rustorb_typesupport::md5(name.as_bytes());
    [digest[0], digest[1], digest[2], digest[3]]
}

// ---------- Flags --------------------------------------------------------

macro_rules! flags {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $flag:ident = $bit:expr),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
        pub struct $name(pub u16);

        impl $name {
            $( $(#[$fmeta])* pub const $flag: $name = $name(1 << $bit); )*

            /// Whether every flag of `other` is set.
            pub fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }
        }

        impl std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }

        impl std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }

        impl Xcdr2 for $name {
            fn encode(&self, enc: &mut Encoder) -> Result<()> {
                enc.write_u16(self.0);
                Ok(())
            }
            fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
                Ok($name(dec.read_u16()?))
            }
        }
    };
}

flags! {
    /// `MemberFlag`: flags of members and elements that affect
    /// assignability.
    MemberFlag {
        TRY_CONSTRUCT1 = 0,
        TRY_CONSTRUCT2 = 1,
        IS_EXTERNAL = 2,
        IS_OPTIONAL = 3,
        IS_MUST_UNDERSTAND = 4,
        IS_KEY = 5,
        IS_DEFAULT = 6,
    }
}

flags! {
    /// `TypeFlag`: flags of type declarations that affect assignability.
    TypeFlag {
        IS_FINAL = 0,
        IS_APPENDABLE = 1,
        IS_MUTABLE = 2,
        IS_NESTED = 3,
        IS_AUTOID_HASH = 4,
    }
}

impl MemberFlag {
    /// `@try_construct(DISCARD)`, the default (7.2.2.7).
    pub const TRY_CONSTRUCT_DISCARD: MemberFlag = MemberFlag::TRY_CONSTRUCT1;
    /// `@try_construct(USE_DEFAULT)`.
    pub const TRY_CONSTRUCT_USE_DEFAULT: MemberFlag = MemberFlag::TRY_CONSTRUCT2;
    /// `@try_construct(TRIM)`.
    pub const TRY_CONSTRUCT_TRIM: MemberFlag = MemberFlag(3);
}

pub type CollectionElementFlag = MemberFlag;
pub type StructMemberFlag = MemberFlag;
pub type UnionMemberFlag = MemberFlag;
pub type UnionDiscriminatorFlag = MemberFlag;
pub type EnumeratedLiteralFlag = MemberFlag;
pub type AnnotationParameterFlag = MemberFlag;
pub type AliasMemberFlag = MemberFlag;
pub type BitflagFlag = MemberFlag;
pub type BitsetMemberFlag = MemberFlag;
/// `MemberFlagMinimalMask`.
pub const MEMBER_FLAG_MINIMAL_MASK: u16 = 0x003f;

pub type StructTypeFlag = TypeFlag;
pub type UnionTypeFlag = TypeFlag;
pub type CollectionTypeFlag = TypeFlag;
pub type AnnotationTypeFlag = TypeFlag;
pub type AliasTypeFlag = TypeFlag;
pub type EnumTypeFlag = TypeFlag;
pub type BitmaskTypeFlag = TypeFlag;
pub type BitsetTypeFlag = TypeFlag;
/// `TypeFlagMinimalMask`.
pub const TYPE_FLAG_MINIMAL_MASK: u16 = 0x0007;

// ---------- TypeObjectHashId ------------------------------------------

/// `TypeObjectHashId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TypeObjectHashId {
    Complete(EquivalenceHash),
    Minimal(EquivalenceHash),
}

impl Xcdr2 for TypeObjectHashId {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let (d, hash) = match self {
            TypeObjectHashId::Complete(h) => (EK_COMPLETE, h),
            TypeObjectHashId::Minimal(h) => (EK_MINIMAL, h),
        };
        enc.write_octet(d);
        enc.write_octets(hash);
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        match dec.read_octet()? {
            EK_COMPLETE => Ok(TypeObjectHashId::Complete(Xcdr2::decode(dec)?)),
            EK_MINIMAL => Ok(TypeObjectHashId::Minimal(Xcdr2::decode(dec)?)),
            _ => Err(Error::Invalid("TypeObjectHashId discriminator")),
        }
    }
}

// ---------- TypeIdentifier ----------------------------------------------

xstruct! {
    /// `StringSTypeDefn`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
    final StringSTypeDefn { bound: SBound }
}

xstruct! {
    /// `StringLTypeDefn`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
    final StringLTypeDefn { bound: LBound }
}

xstruct! {
    /// `PlainCollectionHeader`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
    final PlainCollectionHeader {
        equiv_kind: EquivalenceKind,
        element_flags: CollectionElementFlag,
    }
}

xstruct! {
    /// `PlainSequenceSElemDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainSequenceSElemDefn {
        header: PlainCollectionHeader,
        bound: SBound,
        element_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `PlainSequenceLElemDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainSequenceLElemDefn {
        header: PlainCollectionHeader,
        bound: LBound,
        element_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `PlainArraySElemDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainArraySElemDefn {
        header: PlainCollectionHeader,
        array_bound_seq: SBoundSeq,
        element_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `PlainArrayLElemDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainArrayLElemDefn {
        header: PlainCollectionHeader,
        array_bound_seq: LBoundSeq,
        element_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `PlainMapSTypeDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainMapSTypeDefn {
        header: PlainCollectionHeader,
        bound: SBound,
        element_identifier: Box<TypeIdentifier>,
        key_flags: CollectionElementFlag,
        key_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `PlainMapLTypeDefn`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    final PlainMapLTypeDefn {
        header: PlainCollectionHeader,
        bound: LBound,
        element_identifier: Box<TypeIdentifier>,
        key_flags: CollectionElementFlag,
        key_identifier: Box<TypeIdentifier>,
    }
}

xstruct! {
    /// `StronglyConnectedComponentId`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    appendable StronglyConnectedComponentId {
        sc_component_id: TypeObjectHashId,
        scc_length: i32,
        scc_index: i32,
    }
}

/// `TypeIdentifier`: identifies a type, by describing it when it is a
/// plain type and by a hash of its TypeObject otherwise (7.3.4.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TypeIdentifier {
    /// `TK_NONE` or a primitive type: the discriminator is all there is.
    Primitive(TypeKind),
    /// `TI_STRING8_SMALL` (`wide` false) or `TI_STRING16_SMALL`.
    StringSmall { wide: bool, defn: StringSTypeDefn },
    /// `TI_STRING8_LARGE` or `TI_STRING16_LARGE`.
    StringLarge { wide: bool, defn: StringLTypeDefn },
    /// `TI_PLAIN_SEQUENCE_SMALL`.
    SeqSmall(PlainSequenceSElemDefn),
    /// `TI_PLAIN_SEQUENCE_LARGE`.
    SeqLarge(PlainSequenceLElemDefn),
    /// `TI_PLAIN_ARRAY_SMALL`.
    ArraySmall(PlainArraySElemDefn),
    /// `TI_PLAIN_ARRAY_LARGE`.
    ArrayLarge(PlainArrayLElemDefn),
    /// `TI_PLAIN_MAP_SMALL`.
    MapSmall(PlainMapSTypeDefn),
    /// `TI_PLAIN_MAP_LARGE`.
    MapLarge(PlainMapLTypeDefn),
    /// `TI_STRONGLY_CONNECTED_COMPONENT`.
    StronglyConnectedComponent(StronglyConnectedComponentId),
    /// `EK_MINIMAL`: the hash of a minimal TypeObject.
    Minimal(EquivalenceHash),
    /// `EK_COMPLETE`: the hash of a complete TypeObject.
    Complete(EquivalenceHash),
    /// A discriminator this version does not know (`extended_defn`).
    Extended(u8),
}

impl Default for TypeIdentifier {
    fn default() -> Self {
        TypeIdentifier::Primitive(TK_NONE)
    }
}

impl TypeIdentifier {
    /// The discriminator.
    pub fn discriminator(&self) -> u8 {
        match self {
            TypeIdentifier::Primitive(k) => *k,
            TypeIdentifier::StringSmall { wide: false, .. } => TI_STRING8_SMALL,
            TypeIdentifier::StringSmall { wide: true, .. } => TI_STRING16_SMALL,
            TypeIdentifier::StringLarge { wide: false, .. } => TI_STRING8_LARGE,
            TypeIdentifier::StringLarge { wide: true, .. } => TI_STRING16_LARGE,
            TypeIdentifier::SeqSmall(_) => TI_PLAIN_SEQUENCE_SMALL,
            TypeIdentifier::SeqLarge(_) => TI_PLAIN_SEQUENCE_LARGE,
            TypeIdentifier::ArraySmall(_) => TI_PLAIN_ARRAY_SMALL,
            TypeIdentifier::ArrayLarge(_) => TI_PLAIN_ARRAY_LARGE,
            TypeIdentifier::MapSmall(_) => TI_PLAIN_MAP_SMALL,
            TypeIdentifier::MapLarge(_) => TI_PLAIN_MAP_LARGE,
            TypeIdentifier::StronglyConnectedComponent(_) => TI_STRONGLY_CONNECTED_COMPONENT,
            TypeIdentifier::Minimal(_) => EK_MINIMAL,
            TypeIdentifier::Complete(_) => EK_COMPLETE,
            TypeIdentifier::Extended(d) => *d,
        }
    }

    /// Whether this is a direct hash TypeIdentifier (7.3.4.6.3): one that
    /// has a TypeObject.
    pub fn is_direct_hash(&self) -> bool {
        matches!(
            self,
            TypeIdentifier::Minimal(_)
                | TypeIdentifier::Complete(_)
                | TypeIdentifier::StronglyConnectedComponent(_)
        )
    }

    /// The equivalence kind of a plain collection's header, if this is
    /// one.
    pub fn plain_collection_header(&self) -> Option<&PlainCollectionHeader> {
        match self {
            TypeIdentifier::SeqSmall(d) => Some(&d.header),
            TypeIdentifier::SeqLarge(d) => Some(&d.header),
            TypeIdentifier::ArraySmall(d) => Some(&d.header),
            TypeIdentifier::ArrayLarge(d) => Some(&d.header),
            TypeIdentifier::MapSmall(d) => Some(&d.header),
            TypeIdentifier::MapLarge(d) => Some(&d.header),
            _ => None,
        }
    }

    /// Whether this TypeIdentifier fully describes its type (7.3.4.6.1):
    /// a primitive, a string, or a plain collection of such types.
    pub fn is_fully_descriptive(&self) -> bool {
        match self {
            TypeIdentifier::Primitive(_)
            | TypeIdentifier::StringSmall { .. }
            | TypeIdentifier::StringLarge { .. } => true,
            other => other
                .plain_collection_header()
                .is_some_and(|h| h.equiv_kind == EK_BOTH),
        }
    }
}

impl Xcdr2 for TypeIdentifier {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_octet(self.discriminator());
        match self {
            TypeIdentifier::Primitive(_) => {}
            TypeIdentifier::StringSmall { defn, .. } => defn.encode(enc)?,
            TypeIdentifier::StringLarge { defn, .. } => defn.encode(enc)?,
            TypeIdentifier::SeqSmall(d) => d.encode(enc)?,
            TypeIdentifier::SeqLarge(d) => d.encode(enc)?,
            TypeIdentifier::ArraySmall(d) => d.encode(enc)?,
            TypeIdentifier::ArrayLarge(d) => d.encode(enc)?,
            TypeIdentifier::MapSmall(d) => d.encode(enc)?,
            TypeIdentifier::MapLarge(d) => d.encode(enc)?,
            TypeIdentifier::StronglyConnectedComponent(d) => d.encode(enc)?,
            TypeIdentifier::Minimal(h) | TypeIdentifier::Complete(h) => enc.write_octets(h),
            TypeIdentifier::Extended(_) => encode_empty_mutable(enc)?,
        }
        Ok(())
    }

    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let d = dec.read_octet()?;
        Ok(match d {
            TK_NONE => TypeIdentifier::Primitive(d),
            k if is_primitive_kind(k) => TypeIdentifier::Primitive(k),
            TI_STRING8_SMALL | TI_STRING16_SMALL => TypeIdentifier::StringSmall {
                wide: d == TI_STRING16_SMALL,
                defn: Xcdr2::decode(dec)?,
            },
            TI_STRING8_LARGE | TI_STRING16_LARGE => TypeIdentifier::StringLarge {
                wide: d == TI_STRING16_LARGE,
                defn: Xcdr2::decode(dec)?,
            },
            TI_PLAIN_SEQUENCE_SMALL => TypeIdentifier::SeqSmall(Xcdr2::decode(dec)?),
            TI_PLAIN_SEQUENCE_LARGE => TypeIdentifier::SeqLarge(Xcdr2::decode(dec)?),
            TI_PLAIN_ARRAY_SMALL => TypeIdentifier::ArraySmall(Xcdr2::decode(dec)?),
            TI_PLAIN_ARRAY_LARGE => TypeIdentifier::ArrayLarge(Xcdr2::decode(dec)?),
            TI_PLAIN_MAP_SMALL => TypeIdentifier::MapSmall(Xcdr2::decode(dec)?),
            TI_PLAIN_MAP_LARGE => TypeIdentifier::MapLarge(Xcdr2::decode(dec)?),
            TI_STRONGLY_CONNECTED_COMPONENT => {
                TypeIdentifier::StronglyConnectedComponent(Xcdr2::decode(dec)?)
            }
            EK_MINIMAL => TypeIdentifier::Minimal(Xcdr2::decode(dec)?),
            EK_COMPLETE => TypeIdentifier::Complete(Xcdr2::decode(dec)?),
            other => {
                skip_mutable(dec)?;
                TypeIdentifier::Extended(other)
            }
        })
    }
}

pub type TypeIdentifierSeq = Vec<TypeIdentifier>;

// ---------- Annotation usage ---------------------------------------------

/// `AnnotationParameterValue`.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationParameterValue {
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
    Enumerated(i32),
    String8(String),
    String16(String),
    /// A discriminator this version does not know (`extended_value`).
    Extended(u8),
}

impl Xcdr2 for AnnotationParameterValue {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        use AnnotationParameterValue as V;
        match self {
            V::Boolean(v) => {
                enc.write_octet(TK_BOOLEAN);
                enc.write_bool(*v);
            }
            V::Byte(v) => {
                enc.write_octet(TK_BYTE);
                enc.write_octet(*v);
            }
            V::Int8(v) => {
                enc.write_octet(TK_INT8);
                enc.write_octet(*v as u8);
            }
            V::Uint8(v) => {
                enc.write_octet(TK_UINT8);
                enc.write_octet(*v);
            }
            V::Int16(v) => {
                enc.write_octet(TK_INT16);
                enc.write_i16(*v);
            }
            V::Uint16(v) => {
                enc.write_octet(TK_UINT16);
                enc.write_u16(*v);
            }
            V::Int32(v) => {
                enc.write_octet(TK_INT32);
                enc.write_i32(*v);
            }
            V::Uint32(v) => {
                enc.write_octet(TK_UINT32);
                enc.write_u32(*v);
            }
            V::Int64(v) => {
                enc.write_octet(TK_INT64);
                enc.write_i64(*v);
            }
            V::Uint64(v) => {
                enc.write_octet(TK_UINT64);
                enc.write_u64(*v);
            }
            V::Float32(v) => {
                enc.write_octet(TK_FLOAT32);
                enc.write_f32(*v);
            }
            V::Float64(v) => {
                enc.write_octet(TK_FLOAT64);
                enc.write_f64(*v);
            }
            V::Float128(v) => {
                enc.write_octet(TK_FLOAT128);
                enc.write_long_double(*v);
            }
            V::Char8(v) => {
                enc.write_octet(TK_CHAR8);
                enc.write_char(*v);
            }
            V::Char16(v) => {
                enc.write_octet(TK_CHAR16);
                enc.write_wchar(*v)?;
            }
            V::Enumerated(v) => {
                enc.write_octet(TK_ENUM);
                enc.write_i32(*v);
            }
            V::String8(v) => {
                enc.write_octet(TK_STRING8);
                enc.write_string(v)?;
            }
            V::String16(v) => {
                enc.write_octet(TK_STRING16);
                enc.write_wstring(v)?;
            }
            V::Extended(d) => {
                enc.write_octet(*d);
                encode_empty_mutable(enc)?;
            }
        }
        Ok(())
    }

    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        use AnnotationParameterValue as V;
        Ok(match dec.read_octet()? {
            TK_BOOLEAN => V::Boolean(dec.read_bool()?),
            TK_BYTE => V::Byte(dec.read_octet()?),
            TK_INT8 => V::Int8(dec.read_octet()? as i8),
            TK_UINT8 => V::Uint8(dec.read_octet()?),
            TK_INT16 => V::Int16(dec.read_i16()?),
            TK_UINT16 => V::Uint16(dec.read_u16()?),
            TK_INT32 => V::Int32(dec.read_i32()?),
            TK_UINT32 => V::Uint32(dec.read_u32()?),
            TK_INT64 => V::Int64(dec.read_i64()?),
            TK_UINT64 => V::Uint64(dec.read_u64()?),
            TK_FLOAT32 => V::Float32(dec.read_f32()?),
            TK_FLOAT64 => V::Float64(dec.read_f64()?),
            TK_FLOAT128 => V::Float128(dec.read_long_double()?),
            TK_CHAR8 => V::Char8(dec.read_char()?),
            TK_CHAR16 => V::Char16(dec.read_wchar()?),
            TK_ENUM => V::Enumerated(dec.read_i32()?),
            TK_STRING8 => V::String8(dec.read_string()?),
            TK_STRING16 => V::String16(dec.read_wstring()?),
            other => {
                skip_mutable(dec)?;
                V::Extended(other)
            }
        })
    }
}

xstruct! {
    /// `AppliedAnnotationParameter`.
    #[derive(Debug, Clone, PartialEq)]
    appendable AppliedAnnotationParameter {
        paramname_hash: NameHash,
        value: AnnotationParameterValue,
    }
}

/// Sorted by `paramname_hash`.
pub type AppliedAnnotationParameterSeq = Vec<AppliedAnnotationParameter>;

xstruct! {
    /// `AppliedAnnotation`.
    #[derive(Debug, Clone, PartialEq)]
    appendable AppliedAnnotation {
        annotation_typeid: TypeIdentifier,
        param_seq: Option<AppliedAnnotationParameterSeq>,
    }
}

/// Sorted by `annotation_typeid`.
pub type AppliedAnnotationSeq = Vec<AppliedAnnotation>;

xstruct! {
    /// `AppliedVerbatimAnnotation`: `@verbatim(placement, language, text)`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final AppliedVerbatimAnnotation {
        placement: String,
        language: String,
        text: String,
    }
}

// ---------- Aggregate types ----------------------------------------------

xstruct! {
    /// `AppliedBuiltinMemberAnnotations`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable AppliedBuiltinMemberAnnotations {
        /// `@unit`.
        unit: Option<String>,
        /// `@min`, `@range`.
        min: Option<AnnotationParameterValue>,
        /// `@max`, `@range`.
        max: Option<AnnotationParameterValue>,
        /// `@hashid`.
        hash_id: Option<String>,
    }
}

xstruct! {
    /// `CommonStructMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonStructMember {
        member_id: MemberId,
        member_flags: StructMemberFlag,
        member_type_id: TypeIdentifier,
    }
}

xstruct! {
    /// `CompleteMemberDetail`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteMemberDetail {
        name: MemberName,
        ann_builtin: Option<AppliedBuiltinMemberAnnotations>,
        ann_custom: Option<AppliedAnnotationSeq>,
    }
}

xstruct! {
    /// `MinimalMemberDetail`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalMemberDetail { name_hash: NameHash }
}

xstruct! {
    /// `CompleteStructMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteStructMember {
        common: CommonStructMember,
        detail: CompleteMemberDetail,
    }
}

/// Ordered by member index.
pub type CompleteStructMemberSeq = Vec<CompleteStructMember>;

xstruct! {
    /// `MinimalStructMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalStructMember {
        common: CommonStructMember,
        detail: MinimalMemberDetail,
    }
}

/// Ordered by member index.
pub type MinimalStructMemberSeq = Vec<MinimalStructMember>;

xstruct! {
    /// `AppliedBuiltinTypeAnnotations`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable AppliedBuiltinTypeAnnotations {
        verbatim: Option<AppliedVerbatimAnnotation>,
    }
}

xstruct! {
    /// `MinimalTypeDetail`: empty.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalTypeDetail {}
}

xstruct! {
    /// `CompleteTypeDetail`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteTypeDetail {
        ann_builtin: Option<AppliedBuiltinTypeAnnotations>,
        ann_custom: Option<AppliedAnnotationSeq>,
        type_name: QualifiedTypeName,
    }
}

xstruct! {
    /// `CompleteStructHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteStructHeader {
        base_type: TypeIdentifier,
        detail: CompleteTypeDetail,
    }
}

xstruct! {
    /// `MinimalStructHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalStructHeader {
        base_type: TypeIdentifier,
        detail: MinimalTypeDetail,
    }
}

xstruct! {
    /// `CompleteStructType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteStructType {
        struct_flags: StructTypeFlag,
        header: CompleteStructHeader,
        member_seq: CompleteStructMemberSeq,
    }
}

xstruct! {
    /// `MinimalStructType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalStructType {
        struct_flags: StructTypeFlag,
        header: MinimalStructHeader,
        member_seq: MinimalStructMemberSeq,
    }
}

// ---------- Union ---------------------------------------------------------

/// Case labels, ordered by value.
pub type UnionCaseLabelSeq = Vec<i32>;

xstruct! {
    /// `CommonUnionMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonUnionMember {
        member_id: MemberId,
        member_flags: UnionMemberFlag,
        type_id: TypeIdentifier,
        label_seq: UnionCaseLabelSeq,
    }
}

xstruct! {
    /// `CompleteUnionMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteUnionMember {
        common: CommonUnionMember,
        detail: CompleteMemberDetail,
    }
}

pub type CompleteUnionMemberSeq = Vec<CompleteUnionMember>;

xstruct! {
    /// `MinimalUnionMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalUnionMember {
        common: CommonUnionMember,
        detail: MinimalMemberDetail,
    }
}

pub type MinimalUnionMemberSeq = Vec<MinimalUnionMember>;

xstruct! {
    /// `CommonDiscriminatorMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonDiscriminatorMember {
        member_flags: UnionDiscriminatorFlag,
        type_id: TypeIdentifier,
    }
}

xstruct! {
    /// `CompleteDiscriminatorMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteDiscriminatorMember {
        common: CommonDiscriminatorMember,
        ann_builtin: Option<AppliedBuiltinTypeAnnotations>,
        ann_custom: Option<AppliedAnnotationSeq>,
    }
}

xstruct! {
    /// `MinimalDiscriminatorMember`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalDiscriminatorMember { common: CommonDiscriminatorMember }
}

xstruct! {
    /// `CompleteUnionHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteUnionHeader { detail: CompleteTypeDetail }
}

xstruct! {
    /// `MinimalUnionHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalUnionHeader { detail: MinimalTypeDetail }
}

xstruct! {
    /// `CompleteUnionType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteUnionType {
        union_flags: UnionTypeFlag,
        header: CompleteUnionHeader,
        discriminator: CompleteDiscriminatorMember,
        member_seq: CompleteUnionMemberSeq,
    }
}

xstruct! {
    /// `MinimalUnionType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalUnionType {
        union_flags: UnionTypeFlag,
        header: MinimalUnionHeader,
        discriminator: MinimalDiscriminatorMember,
        member_seq: MinimalUnionMemberSeq,
    }
}

// ---------- Annotation ----------------------------------------------------

xstruct! {
    /// `CommonAnnotationParameter`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonAnnotationParameter {
        member_flags: AnnotationParameterFlag,
        member_type_id: TypeIdentifier,
    }
}

xstruct! {
    /// `CompleteAnnotationParameter`.
    #[derive(Debug, Clone, PartialEq)]
    appendable CompleteAnnotationParameter {
        common: CommonAnnotationParameter,
        name: MemberName,
        default_value: AnnotationParameterValue,
    }
}

pub type CompleteAnnotationParameterSeq = Vec<CompleteAnnotationParameter>;

xstruct! {
    /// `MinimalAnnotationParameter`.
    #[derive(Debug, Clone, PartialEq)]
    appendable MinimalAnnotationParameter {
        common: CommonAnnotationParameter,
        name_hash: NameHash,
        default_value: AnnotationParameterValue,
    }
}

pub type MinimalAnnotationParameterSeq = Vec<MinimalAnnotationParameter>;

xstruct! {
    /// `CompleteAnnotationHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteAnnotationHeader { annotation_name: QualifiedTypeName }
}

xstruct! {
    /// `MinimalAnnotationHeader`: empty.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalAnnotationHeader {}
}

xstruct! {
    /// `CompleteAnnotationType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteAnnotationType {
        annotation_flag: AnnotationTypeFlag,
        header: CompleteAnnotationHeader,
        member_seq: CompleteAnnotationParameterSeq,
    }
}

xstruct! {
    /// `MinimalAnnotationType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalAnnotationType {
        annotation_flag: AnnotationTypeFlag,
        header: MinimalAnnotationHeader,
        member_seq: MinimalAnnotationParameterSeq,
    }
}

// ---------- Alias ---------------------------------------------------------

xstruct! {
    /// `CommonAliasBody`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonAliasBody {
        related_flags: AliasMemberFlag,
        related_type: TypeIdentifier,
    }
}

xstruct! {
    /// `CompleteAliasBody`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteAliasBody {
        common: CommonAliasBody,
        ann_builtin: Option<AppliedBuiltinMemberAnnotations>,
        ann_custom: Option<AppliedAnnotationSeq>,
    }
}

xstruct! {
    /// `MinimalAliasBody`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalAliasBody { common: CommonAliasBody }
}

xstruct! {
    /// `CompleteAliasHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteAliasHeader { detail: CompleteTypeDetail }
}

xstruct! {
    /// `MinimalAliasHeader`: empty.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalAliasHeader {}
}

xstruct! {
    /// `CompleteAliasType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteAliasType {
        alias_flags: AliasTypeFlag,
        header: CompleteAliasHeader,
        body: CompleteAliasBody,
    }
}

xstruct! {
    /// `MinimalAliasType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalAliasType {
        alias_flags: AliasTypeFlag,
        header: MinimalAliasHeader,
        body: MinimalAliasBody,
    }
}

// ---------- Collections ---------------------------------------------------

xstruct! {
    /// `CompleteElementDetail`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteElementDetail {
        ann_builtin: Option<AppliedBuiltinMemberAnnotations>,
        ann_custom: Option<AppliedAnnotationSeq>,
    }
}

xstruct! {
    /// `CommonCollectionElement`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonCollectionElement {
        element_flags: CollectionElementFlag,
        r#type: TypeIdentifier,
    }
}

xstruct! {
    /// `CompleteCollectionElement`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteCollectionElement {
        common: CommonCollectionElement,
        detail: CompleteElementDetail,
    }
}

xstruct! {
    /// `MinimalCollectionElement`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalCollectionElement { common: CommonCollectionElement }
}

xstruct! {
    /// `CommonCollectionHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonCollectionHeader { bound: LBound }
}

xstruct! {
    /// `CompleteCollectionHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteCollectionHeader {
        common: CommonCollectionHeader,
        /// Absent for anonymous collections.
        detail: Option<CompleteTypeDetail>,
    }
}

xstruct! {
    /// `MinimalCollectionHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalCollectionHeader { common: CommonCollectionHeader }
}

xstruct! {
    /// `CompleteSequenceType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteSequenceType {
        collection_flag: CollectionTypeFlag,
        header: CompleteCollectionHeader,
        element: CompleteCollectionElement,
    }
}

xstruct! {
    /// `MinimalSequenceType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalSequenceType {
        collection_flag: CollectionTypeFlag,
        header: MinimalCollectionHeader,
        element: MinimalCollectionElement,
    }
}

xstruct! {
    /// `CommonArrayHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonArrayHeader { bound_seq: LBoundSeq }
}

xstruct! {
    /// `CompleteArrayHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteArrayHeader {
        common: CommonArrayHeader,
        detail: CompleteTypeDetail,
    }
}

xstruct! {
    /// `MinimalArrayHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalArrayHeader { common: CommonArrayHeader }
}

xstruct! {
    /// `CompleteArrayType`. APPENDABLE in Annex B, unlike
    /// [`MinimalArrayType`].
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteArrayType {
        collection_flag: CollectionTypeFlag,
        header: CompleteArrayHeader,
        element: CompleteCollectionElement,
    }
}

xstruct! {
    /// `MinimalArrayType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalArrayType {
        collection_flag: CollectionTypeFlag,
        header: MinimalArrayHeader,
        element: MinimalCollectionElement,
    }
}

xstruct! {
    /// `CompleteMapType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteMapType {
        collection_flag: CollectionTypeFlag,
        header: CompleteCollectionHeader,
        key: CompleteCollectionElement,
        element: CompleteCollectionElement,
    }
}

xstruct! {
    /// `MinimalMapType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalMapType {
        collection_flag: CollectionTypeFlag,
        header: MinimalCollectionHeader,
        key: MinimalCollectionElement,
        element: MinimalCollectionElement,
    }
}

// ---------- Enumeration ---------------------------------------------------

/// `BitBound`.
pub type BitBound = u16;

xstruct! {
    /// `CommonEnumeratedLiteral`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CommonEnumeratedLiteral {
        value: i32,
        flags: EnumeratedLiteralFlag,
    }
}

xstruct! {
    /// `CompleteEnumeratedLiteral`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteEnumeratedLiteral {
        common: CommonEnumeratedLiteral,
        detail: CompleteMemberDetail,
    }
}

/// Ordered by value.
pub type CompleteEnumeratedLiteralSeq = Vec<CompleteEnumeratedLiteral>;

xstruct! {
    /// `MinimalEnumeratedLiteral`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalEnumeratedLiteral {
        common: CommonEnumeratedLiteral,
        detail: MinimalMemberDetail,
    }
}

/// Ordered by value.
pub type MinimalEnumeratedLiteralSeq = Vec<MinimalEnumeratedLiteral>;

xstruct! {
    /// `CommonEnumeratedHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonEnumeratedHeader { bit_bound: BitBound }
}

xstruct! {
    /// `CompleteEnumeratedHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteEnumeratedHeader {
        common: CommonEnumeratedHeader,
        detail: CompleteTypeDetail,
    }
}

xstruct! {
    /// `MinimalEnumeratedHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalEnumeratedHeader { common: CommonEnumeratedHeader }
}

xstruct! {
    /// `CompleteEnumeratedType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CompleteEnumeratedType {
        enum_flags: EnumTypeFlag,
        header: CompleteEnumeratedHeader,
        literal_seq: CompleteEnumeratedLiteralSeq,
    }
}

xstruct! {
    /// `MinimalEnumeratedType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final MinimalEnumeratedType {
        enum_flags: EnumTypeFlag,
        header: MinimalEnumeratedHeader,
        literal_seq: MinimalEnumeratedLiteralSeq,
    }
}

// ---------- Bitmask -------------------------------------------------------

xstruct! {
    /// `CommonBitflag`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonBitflag {
        position: u16,
        flags: BitflagFlag,
    }
}

xstruct! {
    /// `CompleteBitflag`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteBitflag {
        common: CommonBitflag,
        detail: CompleteMemberDetail,
    }
}

/// Ordered by position.
pub type CompleteBitflagSeq = Vec<CompleteBitflag>;

xstruct! {
    /// `MinimalBitflag`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalBitflag {
        common: CommonBitflag,
        detail: MinimalMemberDetail,
    }
}

/// Ordered by position.
pub type MinimalBitflagSeq = Vec<MinimalBitflag>;

xstruct! {
    /// `CommonBitmaskHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonBitmaskHeader { bit_bound: BitBound }
}

/// `CompleteBitmaskHeader`.
pub type CompleteBitmaskHeader = CompleteEnumeratedHeader;
/// `MinimalBitmaskHeader`.
pub type MinimalBitmaskHeader = MinimalEnumeratedHeader;

xstruct! {
    /// `CompleteBitmaskType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteBitmaskType {
        bitmask_flags: BitmaskTypeFlag,
        header: CompleteBitmaskHeader,
        flag_seq: CompleteBitflagSeq,
    }
}

xstruct! {
    /// `MinimalBitmaskType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalBitmaskType {
        bitmask_flags: BitmaskTypeFlag,
        header: MinimalBitmaskHeader,
        flag_seq: MinimalBitflagSeq,
    }
}

// ---------- Bitset --------------------------------------------------------

xstruct! {
    /// `CommonBitfield`.
    #[derive(Debug, Clone, PartialEq, Default)]
    final CommonBitfield {
        position: u16,
        flags: BitsetMemberFlag,
        bitcount: u8,
        /// A primitive integer type.
        holder_type: TypeKind,
    }
}

xstruct! {
    /// `CompleteBitfield`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteBitfield {
        common: CommonBitfield,
        detail: CompleteMemberDetail,
    }
}

pub type CompleteBitfieldSeq = Vec<CompleteBitfield>;

xstruct! {
    /// `MinimalBitfield`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalBitfield {
        common: CommonBitfield,
        name_hash: NameHash,
    }
}

pub type MinimalBitfieldSeq = Vec<MinimalBitfield>;

xstruct! {
    /// `CompleteBitsetHeader`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteBitsetHeader { detail: CompleteTypeDetail }
}

xstruct! {
    /// `MinimalBitsetHeader`: empty.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalBitsetHeader {}
}

xstruct! {
    /// `CompleteBitsetType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable CompleteBitsetType {
        bitset_flags: BitsetTypeFlag,
        header: CompleteBitsetHeader,
        field_seq: CompleteBitfieldSeq,
    }
}

xstruct! {
    /// `MinimalBitsetType`.
    #[derive(Debug, Clone, PartialEq, Default)]
    appendable MinimalBitsetType {
        bitset_flags: BitsetTypeFlag,
        header: MinimalBitsetHeader,
        field_seq: MinimalBitfieldSeq,
    }
}

// ---------- TypeObject ----------------------------------------------------

macro_rules! type_object_union {
    ($(#[$meta:meta])* $name:ident {
        $($variant:ident($ty:ty) = $tk:ident),* $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[allow(clippy::large_enum_variant)] // Few of them, built once.
        pub enum $name {
            $( $variant($ty), )*
            /// A type kind this version does not know (`extended_type`).
            Extended(u8),
        }

        impl $name {
            /// The discriminator: the type kind.
            pub fn kind(&self) -> TypeKind {
                match self {
                    $( $name::$variant(_) => $tk, )*
                    $name::Extended(d) => *d,
                }
            }
        }

        impl Xcdr2 for $name {
            fn encode(&self, enc: &mut Encoder) -> Result<()> {
                enc.write_octet(self.kind());
                match self {
                    $( $name::$variant(v) => v.encode(enc), )*
                    $name::Extended(_) => encode_empty_mutable(enc),
                }
            }
            fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
                Ok(match dec.read_octet()? {
                    $( $tk => $name::$variant(Xcdr2::decode(dec)?), )*
                    other => {
                        skip_mutable(dec)?;
                        $name::Extended(other)
                    }
                })
            }
        }
    };
}

type_object_union! {
    /// `CompleteTypeObject`.
    CompleteTypeObject {
        Alias(CompleteAliasType) = TK_ALIAS,
        Annotation(CompleteAnnotationType) = TK_ANNOTATION,
        Struct(CompleteStructType) = TK_STRUCTURE,
        Union(CompleteUnionType) = TK_UNION,
        Bitset(CompleteBitsetType) = TK_BITSET,
        Sequence(CompleteSequenceType) = TK_SEQUENCE,
        Array(CompleteArrayType) = TK_ARRAY,
        Map(CompleteMapType) = TK_MAP,
        Enumerated(CompleteEnumeratedType) = TK_ENUM,
        Bitmask(CompleteBitmaskType) = TK_BITMASK,
    }
}

type_object_union! {
    /// `MinimalTypeObject`.
    MinimalTypeObject {
        Alias(MinimalAliasType) = TK_ALIAS,
        Annotation(MinimalAnnotationType) = TK_ANNOTATION,
        Struct(MinimalStructType) = TK_STRUCTURE,
        Union(MinimalUnionType) = TK_UNION,
        Bitset(MinimalBitsetType) = TK_BITSET,
        Sequence(MinimalSequenceType) = TK_SEQUENCE,
        Array(MinimalArrayType) = TK_ARRAY,
        Map(MinimalMapType) = TK_MAP,
        Enumerated(MinimalEnumeratedType) = TK_ENUM,
        Bitmask(MinimalBitmaskType) = TK_BITMASK,
    }
}

/// Adds the direct hash TypeIdentifiers that `id` is or holds (the
/// elements and keys of plain collections) to `out`.
fn push_hashes<'a>(id: &'a TypeIdentifier, out: &mut Vec<&'a TypeIdentifier>) {
    match id {
        TypeIdentifier::SeqSmall(d) => push_hashes(&d.element_identifier, out),
        TypeIdentifier::SeqLarge(d) => push_hashes(&d.element_identifier, out),
        TypeIdentifier::ArraySmall(d) => push_hashes(&d.element_identifier, out),
        TypeIdentifier::ArrayLarge(d) => push_hashes(&d.element_identifier, out),
        TypeIdentifier::MapSmall(d) => {
            push_hashes(&d.key_identifier, out);
            push_hashes(&d.element_identifier, out);
        }
        TypeIdentifier::MapLarge(d) => {
            push_hashes(&d.key_identifier, out);
            push_hashes(&d.element_identifier, out);
        }
        other if other.is_direct_hash() => out.push(other),
        _ => {}
    }
}

macro_rules! referenced_types {
    ($object:expr, $ty:ident, $out:ident) => {
        match $object {
            $ty::Alias(t) => push_hashes(&t.body.common.related_type, $out),
            $ty::Struct(t) => {
                push_hashes(&t.header.base_type, $out);
                for m in &t.member_seq {
                    push_hashes(&m.common.member_type_id, $out);
                }
            }
            $ty::Union(t) => {
                push_hashes(&t.discriminator.common.type_id, $out);
                for m in &t.member_seq {
                    push_hashes(&m.common.type_id, $out);
                }
            }
            $ty::Sequence(t) => push_hashes(&t.element.common.r#type, $out),
            $ty::Array(t) => push_hashes(&t.element.common.r#type, $out),
            $ty::Map(t) => {
                push_hashes(&t.key.common.r#type, $out);
                push_hashes(&t.element.common.r#type, $out);
            }
            $ty::Annotation(_) | $ty::Bitset(_) | $ty::Enumerated(_) | $ty::Bitmask(_) => {}
            $ty::Extended(_) => {}
        }
    };
}

/// `TypeObject`: the description of a type, COMPLETE or MINIMAL
/// (7.3.4.3, 7.3.4.4).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // Few of them, built once.
pub enum TypeObject {
    Complete(CompleteTypeObject),
    Minimal(MinimalTypeObject),
}

impl Xcdr2 for TypeObject {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let mark = enc.begin_dheader();
        match self {
            TypeObject::Complete(v) => {
                enc.write_octet(EK_COMPLETE);
                v.encode(enc)?;
            }
            TypeObject::Minimal(v) => {
                enc.write_octet(EK_MINIMAL);
                v.encode(enc)?;
            }
        }
        enc.end_dheader(mark)?;
        Ok(())
    }

    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let delimited = dec.read_dheader()?;
        let value = match dec.read_octet()? {
            EK_COMPLETE => TypeObject::Complete(Xcdr2::decode(dec)?),
            EK_MINIMAL => TypeObject::Minimal(Xcdr2::decode(dec)?),
            _ => return Err(Error::Invalid("TypeObject discriminator")),
        };
        dec.end_dheader(delimited)?;
        Ok(value)
    }
}

impl TypeObject {
    /// The serialization the hash is computed on: XCDR2, little endian
    /// (7.3.4.5).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        xcdr2::to_bytes(self, ByteOrder::Little)
    }

    /// Reads a TypeObject serialized in XCDR2 little endian, as
    /// [`to_bytes`](TypeObject::to_bytes) writes it.
    pub fn from_bytes(buf: &[u8]) -> Result<TypeObject> {
        xcdr2::from_bytes(buf, ByteOrder::Little)
    }

    /// The equivalence hash: the first 14 octets of the MD5 of
    /// [`to_bytes`](TypeObject::to_bytes).
    pub fn hash(&self) -> Result<EquivalenceHash> {
        Ok(hash_of(&self.to_bytes()?))
    }

    /// The direct hash TypeIdentifiers of the types this type uses
    /// directly: those of members, base types, aliased types and
    /// collection elements (annotations left out).
    pub fn referenced_types(&self) -> Vec<&TypeIdentifier> {
        let mut out = Vec::new();
        let out_ref = &mut out;
        match self {
            TypeObject::Minimal(m) => referenced_types!(m, MinimalTypeObject, out_ref),
            TypeObject::Complete(c) => referenced_types!(c, CompleteTypeObject, out_ref),
        }
        out
    }

    /// The TypeIdentifier of this TypeObject: `EK_COMPLETE` or
    /// `EK_MINIMAL` and its hash.
    pub fn type_identifier(&self) -> Result<TypeIdentifier> {
        let hash = self.hash()?;
        Ok(match self {
            TypeObject::Complete(_) => TypeIdentifier::Complete(hash),
            TypeObject::Minimal(_) => TypeIdentifier::Minimal(hash),
        })
    }
}

/// The [`EquivalenceHash`] of a serialized TypeObject.
pub fn hash_of(serialized: &[u8]) -> EquivalenceHash {
    let digest = rustorb_typesupport::md5(serialized);
    let mut hash = [0; 14];
    hash.copy_from_slice(&digest[..14]);
    hash
}

pub type TypeObjectSeq = Vec<TypeObject>;
/// Ordered by fully qualified type name.
pub type StronglyConnectedComponent = TypeObjectSeq;

xstruct! {
    /// `TypeIdentifierTypeObjectPair`.
    #[derive(Debug, Clone, PartialEq)]
    final TypeIdentifierTypeObjectPair {
        type_identifier: TypeIdentifier,
        type_object: TypeObject,
    }
}

pub type TypeIdentifierTypeObjectPairSeq = Vec<TypeIdentifierTypeObjectPair>;

xstruct! {
    /// `TypeIdentifierPair`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    final TypeIdentifierPair {
        type_identifier1: TypeIdentifier,
        type_identifier2: TypeIdentifier,
    }
}

pub type TypeIdentifierPairSeq = Vec<TypeIdentifierPair>;

// ---------- TypeInformation (7.6.3.2.1) -----------------------------------

xstruct! {
    /// `TypeIdentfierWithSize` (sic): a TypeIdentifier and the size of its
    /// serialized TypeObject.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
    appendable TypeIdentifierWithSize {
        type_id: TypeIdentifier,
        typeobject_serialized_size: u32,
    }
}

pub type TypeIdentifierWithSizeSeq = Vec<TypeIdentifierWithSize>;

xstruct! {
    /// `TypeIdentifierWithDependencies`.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
    appendable TypeIdentifierWithDependencies {
        typeid_with_size: TypeIdentifierWithSize,
        /// The number of types the type depends on, or -1 when not given.
        dependent_typeid_count: i32,
        /// Some or all of those types.
        dependent_typeids: Vec<TypeIdentifierWithSize>,
    }
}

pub type TypeIdentifierWithDependenciesSeq = Vec<TypeIdentifierWithDependencies>;

/// The member ID of [`TypeInformation::minimal`].
pub const TYPE_INFORMATION_MINIMAL_ID: u32 = 0x1001;
/// The member ID of [`TypeInformation::complete`].
pub const TYPE_INFORMATION_COMPLETE_ID: u32 = 0x1002;

/// `TypeInformation`: the type of an endpoint, as announced in discovery
/// (`PID_TYPE_INFORMATION`). A MUTABLE struct.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TypeInformation {
    pub minimal: TypeIdentifierWithDependencies,
    pub complete: TypeIdentifierWithDependencies,
}

impl Xcdr2 for TypeInformation {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let mark = enc.begin_dheader();
        let m = enc.begin_member(TYPE_INFORMATION_MINIMAL_ID, false)?;
        self.minimal.encode(enc)?;
        enc.end_member(m)?;
        let m = enc.begin_member(TYPE_INFORMATION_COMPLETE_ID, false)?;
        self.complete.encode(enc)?;
        enc.end_member(m)?;
        enc.end_dheader(mark)?;
        Ok(())
    }

    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let delimited = dec.read_dheader()?;
        let mut info = TypeInformation::default();
        while delimited.has_more(dec) {
            let header = dec.read_member_header()?;
            match header.id {
                TYPE_INFORMATION_MINIMAL_ID => info.minimal = Xcdr2::decode(dec)?,
                TYPE_INFORMATION_COMPLETE_ID => info.complete = Xcdr2::decode(dec)?,
                _ if header.must_understand => {
                    return Err(Error::Invalid("unknown must-understand member"));
                }
                _ => {}
            }
            dec.end_member(header)?;
        }
        dec.end_dheader(delimited)?;
        Ok(info)
    }
}

impl TypeInformation {
    /// The serialization of `PID_TYPE_INFORMATION`, without encapsulation:
    /// XCDR2 in `order`.
    pub fn to_bytes(&self, order: ByteOrder) -> Result<Vec<u8>> {
        xcdr2::to_bytes(self, order)
    }

    /// Reads a TypeInformation serialized in XCDR2 with `order`.
    pub fn from_bytes(buf: &[u8], order: ByteOrder) -> Result<TypeInformation> {
        xcdr2::from_bytes(buf, order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Xcdr2 + PartialEq + std::fmt::Debug>(value: &T, order: ByteOrder) -> Vec<u8> {
        let bytes = xcdr2::to_bytes(value, order).unwrap();
        assert_eq!(&xcdr2::from_bytes::<T>(&bytes, order).unwrap(), value);
        bytes
    }

    #[test]
    fn name_hash_of_the_spec_example() {
        // Annex B: "the member name "color" has NameHash {0x70, 0xDD, 0xA5, 0xDF}".
        assert_eq!(name_hash("color"), [0x70, 0xDD, 0xA5, 0xDF]);
    }

    #[test]
    fn primitive_identifiers_are_one_octet() {
        for kind in [
            TK_NONE,
            TK_BOOLEAN,
            TK_INT8,
            TK_UINT8,
            TK_FLOAT128,
            TK_CHAR16,
        ] {
            let id = TypeIdentifier::Primitive(kind);
            assert_eq!(round_trip(&id, ByteOrder::Little), [kind]);
            assert!(id.is_fully_descriptive());
            assert!(!id.is_direct_hash());
        }
    }

    #[test]
    fn identifiers_round_trip() {
        let hash = [7; 14];
        let ids = [
            TypeIdentifier::StringSmall {
                wide: true,
                defn: StringSTypeDefn { bound: 9 },
            },
            TypeIdentifier::StringLarge {
                wide: false,
                defn: StringLTypeDefn { bound: 1000 },
            },
            TypeIdentifier::SeqLarge(PlainSequenceLElemDefn {
                header: PlainCollectionHeader {
                    equiv_kind: EK_MINIMAL,
                    element_flags: MemberFlag::TRY_CONSTRUCT_DISCARD,
                },
                bound: 300,
                element_identifier: Box::new(TypeIdentifier::Minimal(hash)),
            }),
            TypeIdentifier::MapSmall(PlainMapSTypeDefn {
                header: PlainCollectionHeader {
                    equiv_kind: EK_BOTH,
                    element_flags: MemberFlag::TRY_CONSTRUCT_DISCARD,
                },
                bound: 0,
                element_identifier: Box::new(TypeIdentifier::Primitive(TK_INT64)),
                key_flags: MemberFlag::TRY_CONSTRUCT_DISCARD,
                key_identifier: Box::new(TypeIdentifier::StringSmall {
                    wide: false,
                    defn: StringSTypeDefn { bound: 0 },
                }),
            }),
            TypeIdentifier::StronglyConnectedComponent(StronglyConnectedComponentId {
                sc_component_id: TypeObjectHashId::Complete(hash),
                scc_length: 2,
                scc_index: 1,
            }),
            TypeIdentifier::Complete(hash),
        ];
        for id in ids {
            round_trip(&id, ByteOrder::Little);
            round_trip(&id, ByteOrder::Big);
        }
    }

    #[test]
    fn unknown_identifier_kinds_are_skipped() {
        // An extended_defn: a DHEADER and what it delimits.
        let bytes = [0xC0, 0, 0, 0, 0, 0, 0, 4, 1, 2, 3, 4];
        let id: TypeIdentifier = xcdr2::from_bytes(&bytes[..], ByteOrder::Big).unwrap();
        assert_eq!(id, TypeIdentifier::Extended(0xC0));
    }

    #[test]
    fn type_information_in_both_byte_orders() {
        let info = TypeInformation {
            minimal: TypeIdentifierWithDependencies {
                typeid_with_size: TypeIdentifierWithSize {
                    type_id: TypeIdentifier::Minimal([1; 14]),
                    typeobject_serialized_size: 56,
                },
                dependent_typeid_count: -1,
                dependent_typeids: vec![],
            },
            complete: TypeIdentifierWithDependencies {
                typeid_with_size: TypeIdentifierWithSize {
                    type_id: TypeIdentifier::Complete([2; 14]),
                    typeobject_serialized_size: 102,
                },
                dependent_typeid_count: 1,
                dependent_typeids: vec![TypeIdentifierWithSize {
                    type_id: TypeIdentifier::Complete([3; 14]),
                    typeobject_serialized_size: 40,
                }],
            },
        };
        round_trip(&info, ByteOrder::Little);
        round_trip(&info, ByteOrder::Big);
    }

    #[test]
    fn type_information_skips_unknown_members() {
        let info = TypeInformation::default();
        let mut bytes = info.to_bytes(ByteOrder::Little).unwrap();
        // Append a member 0x1003 of 4 octets (LC 2) and fix the DHEADER.
        bytes.extend_from_slice(&(0x2000_1003u32).to_le_bytes());
        bytes.extend_from_slice(&[9, 9, 9, 9]);
        let len = (bytes.len() - 4) as u32;
        bytes[..4].copy_from_slice(&len.to_le_bytes());
        assert_eq!(
            TypeInformation::from_bytes(&bytes, ByteOrder::Little).unwrap(),
            info
        );
        // The same with the must-understand flag is an error.
        let at = bytes.len() - 8;
        bytes[at + 3] |= 0x80;
        assert!(TypeInformation::from_bytes(&bytes, ByteOrder::Little).is_err());
    }

    #[test]
    fn annotation_values_round_trip() {
        use AnnotationParameterValue as V;
        for value in [
            V::Boolean(true),
            V::Int8(-3),
            V::Uint64(u64::MAX),
            V::Float64(1.5),
            V::Char16(0x3042),
            V::Enumerated(7),
            V::String8("m/s".into()),
            V::String16("メートル".into()),
        ] {
            let annotation = AppliedBuiltinMemberAnnotations {
                unit: Some("u".into()),
                min: Some(value.clone()),
                max: Some(value),
                hash_id: None,
            };
            round_trip(&annotation, ByteOrder::Little);
            round_trip(&annotation, ByteOrder::Big);
        }
    }

    #[test]
    fn truncated_data_is_an_error() {
        let object = TypeObject::Minimal(MinimalTypeObject::Struct(MinimalStructType {
            struct_flags: TypeFlag::IS_FINAL,
            header: MinimalStructHeader::default(),
            member_seq: vec![MinimalStructMember::default()],
        }));
        let bytes = object.to_bytes().unwrap();
        for len in 0..bytes.len() {
            assert!(TypeObject::from_bytes(&bytes[..len]).is_err(), "{len}");
        }
    }
}
