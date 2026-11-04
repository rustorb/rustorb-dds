//! Type assignability (DDS-XTypes 1.3, 7.2.4) and the type consistency
//! enforcement of endpoint matching (7.6.3.4).
//!
//! The rules are applied to MINIMAL TypeObjects, so member names are
//! compared by their NameHash. A type is looked up in a
//! [`TypeRegistry`]; one that is not there makes the answer
//! [`Assignable::Unknown`] until its TypeObject is known.
//!
//! What is approximated:
//! - APPENDABLE types count as delimited (7.2.4.2), which holds for XCDR2,
//!   the encoding XTypes 1.2 and later readers request.
//! - `KeyHolder(T)` of a key member's struct or union type is taken as
//!   `T` itself: a nested key struct must be assignable as a whole.
//! - `@non_serialized` members are not in TypeObjects that rustorb builds
//!   or reads, so there is nothing to ignore.
//! - Bitsets, annotations and strongly connected components are only
//!   assignable from the same type.

use std::collections::{BTreeMap, HashSet};

use rustorb_cdr::xcdr::Extensibility;

use crate::registry::TypeRegistry;
use crate::type_object::*;

/// `TypeConsistencyKind` (7.6.3.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TypeConsistencyKind {
    /// `DISALLOW_TYPE_COERCION`: the reader's and the writer's types must
    /// be the same.
    DisallowTypeCoercion,
    /// `ALLOW_TYPE_COERCION`, the default: the reader's type must be
    /// assignable from the writer's.
    #[default]
    AllowTypeCoercion,
}

/// `TypeConsistencyEnforcementQosPolicy` (7.6.3.4), a policy of readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeConsistency {
    /// Whether the types may differ.
    pub kind: TypeConsistencyKind,
    /// Ignore the bounds of sequences and maps (default true).
    pub ignore_sequence_bounds: bool,
    /// Ignore the bounds of strings (default true).
    pub ignore_string_bounds: bool,
    /// Match members by ID only, not also by name (default false).
    pub ignore_member_names: bool,
    /// Refuse a writer type with non-optional members the reader's type
    /// does not have (default false).
    pub prevent_type_widening: bool,
    /// Do not match on type names alone when a side gives no type
    /// information (default false).
    pub force_type_validation: bool,
}

impl Default for TypeConsistency {
    fn default() -> Self {
        TypeConsistency {
            kind: TypeConsistencyKind::AllowTypeCoercion,
            ignore_sequence_bounds: true,
            ignore_string_bounds: true,
            ignore_member_names: false,
            prevent_type_widening: false,
            force_type_validation: false,
        }
    }
}

impl TypeConsistency {
    /// What a reader that does not announce the policy is taken to have:
    /// `DISALLOW_TYPE_COERCION` (7.6.3.4.1).
    pub const NOT_ANNOUNCED: TypeConsistency = TypeConsistency {
        kind: TypeConsistencyKind::DisallowTypeCoercion,
        ignore_sequence_bounds: false,
        ignore_string_bounds: false,
        ignore_member_names: false,
        prevent_type_widening: true,
        force_type_validation: false,
    };

    /// The options in effect: with `DISALLOW_TYPE_COERCION`, as though
    /// `prevent_type_widening` were true and the `ignore_*` false.
    pub fn effective(&self) -> TypeConsistency {
        match self.kind {
            TypeConsistencyKind::AllowTypeCoercion => *self,
            TypeConsistencyKind::DisallowTypeCoercion => TypeConsistency {
                force_type_validation: self.force_type_validation,
                ..TypeConsistency::NOT_ANNOUNCED
            },
        }
    }
}

/// The answer to whether one type is assignable from another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assignable {
    /// It is.
    Yes,
    /// It is not.
    No,
    /// The TypeObjects of these types are needed to tell.
    Unknown(Vec<TypeIdentifier>),
}

impl Assignable {
    fn and(self, other: impl FnOnce() -> Assignable) -> Assignable {
        match self {
            Assignable::Yes => other(),
            Assignable::No => Assignable::No,
            Assignable::Unknown(mut missing) => match other() {
                Assignable::No => Assignable::No,
                Assignable::Yes => Assignable::Unknown(missing),
                Assignable::Unknown(more) => {
                    for m in more {
                        if !missing.contains(&m) {
                            missing.push(m);
                        }
                    }
                    Assignable::Unknown(missing)
                }
            },
        }
    }

    fn from_bool(b: bool) -> Assignable {
        if b { Assignable::Yes } else { Assignable::No }
    }
}

/// Whether the type `t1` (a reader's) is assignable from `t2` (a
/// writer's) under `options` (7.2.4.4). Both are MINIMAL TypeIdentifiers
/// (or identifiers that are not hashes).
pub fn is_assignable(
    t1: &TypeIdentifier,
    t2: &TypeIdentifier,
    registry: &TypeRegistry,
    options: &TypeConsistency,
) -> Assignable {
    let mut cx = Cx {
        registry,
        options: options.effective(),
        assumed: HashSet::new(),
    };
    cx.assignable(t1, t2, false)
}

/// The outcome of the type consistency enforcement between a writer and
/// a reader (7.6.3.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consistency {
    /// The types are consistent.
    Consistent,
    /// They are not: an INCONSISTENT_TOPIC.
    Inconsistent,
    /// The TypeObjects of these types are needed to tell.
    Unknown(Vec<TypeIdentifier>),
}

/// The type of one endpoint, as discovery gives it.
#[derive(Debug, Clone, Copy)]
pub struct EndpointType<'a> {
    /// `type_name`.
    pub name: &'a str,
    /// `type_information`, when announced.
    pub information: Option<&'a TypeInformation>,
}

/// The type consistency enforcement between a writer and a reader
/// (7.6.3.4.2). `consistency` is the reader's policy, or
/// [`TypeConsistency::NOT_ANNOUNCED`] when a remote reader does not
/// announce it.
///
/// Step 1: when both give type information, the reader's type must be
/// assignable from the writer's (or with `DISALLOW_TYPE_COERCION`, the
/// same: the same COMPLETE TypeIdentifier when both have one, else the
/// same MINIMAL one). The type names are not compared then. Step 2:
/// otherwise the type names must be the same, and `force_type_validation`
/// fails the match.
pub fn check_consistency(
    writer: EndpointType<'_>,
    reader: EndpointType<'_>,
    consistency: &TypeConsistency,
    registry: &TypeRegistry,
) -> Consistency {
    let ids = |info: Option<&TypeInformation>| {
        info.map(|i| {
            (
                i.minimal.typeid_with_size.type_id.clone(),
                i.complete.typeid_with_size.type_id.clone(),
            )
        })
        .filter(|(m, c)| *m != TypeIdentifier::default() || *c != TypeIdentifier::default())
    };
    let (Some((wm, wc)), Some((rm, rc))) = (ids(writer.information), ids(reader.information))
    else {
        return if !consistency.force_type_validation && writer.name == reader.name {
            Consistency::Consistent
        } else {
            Consistency::Inconsistent
        };
    };
    let none = TypeIdentifier::default();
    if consistency.kind == TypeConsistencyKind::DisallowTypeCoercion {
        let same = if wc != none && rc != none {
            wc == rc
        } else {
            wm == rm
        };
        return if same {
            Consistency::Consistent
        } else {
            Consistency::Inconsistent
        };
    }
    if wm == none || rm == none {
        // Only COMPLETE identifiers: assignability needs the MINIMAL ones.
        return if wc == rc {
            Consistency::Consistent
        } else {
            Consistency::Inconsistent
        };
    }
    match is_assignable(&rm, &wm, registry, consistency) {
        Assignable::Yes => Consistency::Consistent,
        Assignable::No => Consistency::Inconsistent,
        Assignable::Unknown(missing) => Consistency::Unknown(missing),
    }
}

// ---------- the rules ----------------------------------------------------

/// A type with its aliases resolved.
enum View<'a> {
    Primitive(TypeKind),
    String {
        wide: bool,
        bound: u32,
    },
    Sequence {
        element: &'a TypeIdentifier,
        bound: u32,
    },
    Array {
        element: &'a TypeIdentifier,
        bounds: Vec<u32>,
    },
    Map {
        key: &'a TypeIdentifier,
        element: &'a TypeIdentifier,
        bound: u32,
    },
    Enum(&'a MinimalEnumeratedType),
    Bitmask(&'a MinimalBitmaskType),
    Struct(&'a MinimalStructType),
    Union(&'a MinimalUnionType),
    /// Bitsets, annotations, SCCs, unknown kinds.
    Other,
}

struct Cx<'a> {
    registry: &'a TypeRegistry,
    options: TypeConsistency,
    /// Pairs being compared further up: assumed assignable, which settles
    /// recursive types.
    assumed: HashSet<(TypeIdentifier, TypeIdentifier, bool)>,
}

fn extensibility(flags: TypeFlag) -> Extensibility {
    if flags.contains(TypeFlag::IS_MUTABLE) {
        Extensibility::Mutable
    } else if flags.contains(TypeFlag::IS_APPENDABLE) {
        Extensibility::Appendable
    } else {
        Extensibility::Final
    }
}

/// A member of a struct, with base types expanded.
struct Member<'a> {
    id: MemberId,
    flags: MemberFlag,
    ty: &'a TypeIdentifier,
    name: NameHash,
}

impl<'a> Cx<'a> {
    /// Resolves aliases and hashes.
    fn view(&self, mut id: &'a TypeIdentifier) -> Result<View<'a>, TypeIdentifier> {
        loop {
            return Ok(match id {
                TypeIdentifier::Primitive(k) => View::Primitive(*k),
                TypeIdentifier::StringSmall { wide, defn } => View::String {
                    wide: *wide,
                    bound: defn.bound.into(),
                },
                TypeIdentifier::StringLarge { wide, defn } => View::String {
                    wide: *wide,
                    bound: defn.bound,
                },
                TypeIdentifier::SeqSmall(d) => View::Sequence {
                    element: &d.element_identifier,
                    bound: d.bound.into(),
                },
                TypeIdentifier::SeqLarge(d) => View::Sequence {
                    element: &d.element_identifier,
                    bound: d.bound,
                },
                TypeIdentifier::ArraySmall(d) => View::Array {
                    element: &d.element_identifier,
                    bounds: d.array_bound_seq.iter().map(|b| u32::from(*b)).collect(),
                },
                TypeIdentifier::ArrayLarge(d) => View::Array {
                    element: &d.element_identifier,
                    bounds: d.array_bound_seq.clone(),
                },
                TypeIdentifier::MapSmall(d) => View::Map {
                    key: &d.key_identifier,
                    element: &d.element_identifier,
                    bound: d.bound.into(),
                },
                TypeIdentifier::MapLarge(d) => View::Map {
                    key: &d.key_identifier,
                    element: &d.element_identifier,
                    bound: d.bound,
                },
                TypeIdentifier::Minimal(_) => {
                    let Some(object) = self.registry.minimal(id) else {
                        return Err(id.clone());
                    };
                    match object {
                        MinimalTypeObject::Alias(a) => {
                            id = &a.body.common.related_type;
                            continue;
                        }
                        MinimalTypeObject::Struct(s) => View::Struct(s),
                        MinimalTypeObject::Union(u) => View::Union(u),
                        MinimalTypeObject::Sequence(s) => View::Sequence {
                            element: &s.element.common.r#type,
                            bound: s.header.common.bound,
                        },
                        MinimalTypeObject::Array(a) => View::Array {
                            element: &a.element.common.r#type,
                            bounds: a.header.common.bound_seq.clone(),
                        },
                        MinimalTypeObject::Map(m) => View::Map {
                            key: &m.key.common.r#type,
                            element: &m.element.common.r#type,
                            bound: m.header.common.bound,
                        },
                        MinimalTypeObject::Enumerated(e) => View::Enum(e),
                        MinimalTypeObject::Bitmask(b) => View::Bitmask(b),
                        MinimalTypeObject::Annotation(_)
                        | MinimalTypeObject::Bitset(_)
                        | MinimalTypeObject::Extended(_) => View::Other,
                    }
                }
                _ => View::Other,
            });
        }
    }

    /// Whether `t2` is delimited (7.2.4.2).
    fn delimited(&self, t2: &'a TypeIdentifier) -> Assignable {
        match self.view(t2) {
            Err(missing) => Assignable::Unknown(vec![missing]),
            Ok(v) => match v {
                View::Primitive(_) | View::String { .. } | View::Enum(_) | View::Bitmask(_) => {
                    Assignable::Yes
                }
                View::Sequence { element, .. } | View::Array { element, .. } => {
                    self.delimited(element)
                }
                View::Map { key, element, .. } => {
                    self.delimited(key).and(|| self.delimited(element))
                }
                View::Struct(s) => {
                    Assignable::from_bool(extensibility(s.struct_flags) != Extensibility::Final)
                }
                View::Union(u) => {
                    Assignable::from_bool(extensibility(u.union_flags) != Extensibility::Final)
                }
                View::Other => Assignable::No,
            },
        }
    }

    /// Strong assignability (7.2.4.3).
    fn strongly(&mut self, t1: &'a TypeIdentifier, t2: &'a TypeIdentifier) -> Assignable {
        if t1 == t2 {
            return Assignable::Yes;
        }
        self.assignable(t1, t2, false).and(|| self.delimited(t2))
    }

    /// `t1` is-assignable-from `t2`. `key_erased` when the key members of
    /// `t1` and `t2` are compared as ordinary members.
    fn assignable(
        &mut self,
        t1: &'a TypeIdentifier,
        t2: &'a TypeIdentifier,
        key_erased: bool,
    ) -> Assignable {
        if t1 == t2 {
            return Assignable::Yes;
        }
        let pair = (t1.clone(), t2.clone(), key_erased);
        if self.assumed.contains(&pair) {
            return Assignable::Yes;
        }
        let (v1, v2) = match (self.view(t1), self.view(t2)) {
            (Ok(a), Ok(b)) => (a, b),
            (a, b) => {
                return Assignable::Unknown([a.err(), b.err()].into_iter().flatten().collect());
            }
        };
        self.assumed.insert(pair.clone());
        let result = self.views(v1, v2, key_erased);
        self.assumed.remove(&pair);
        result
    }

    fn views(&mut self, v1: View<'a>, v2: View<'a>, key_erased: bool) -> Assignable {
        let o = self.options;
        let bound_ok = |b1: u32, b2: u32, ignore: bool| ignore || b1 == 0 || (b2 != 0 && b1 >= b2);
        match (v1, v2) {
            (View::Primitive(a), View::Primitive(b)) => Assignable::from_bool(a == b),
            (View::Primitive(k), View::Bitmask(b)) | (View::Bitmask(b), View::Primitive(k)) => {
                Assignable::from_bool(bitmask_holds(k, b.header.common.bit_bound))
            }
            (
                View::String {
                    wide: w1,
                    bound: b1,
                },
                View::String {
                    wide: w2,
                    bound: b2,
                },
            ) => Assignable::from_bool(w1 == w2 && bound_ok(b1, b2, o.ignore_string_bounds)),
            (
                View::Sequence {
                    element: e1,
                    bound: b1,
                },
                View::Sequence {
                    element: e2,
                    bound: b2,
                },
            ) => {
                if !bound_ok(b1, b2, o.ignore_sequence_bounds) {
                    return Assignable::No;
                }
                self.strongly(e1, e2)
            }
            (
                View::Array {
                    element: e1,
                    bounds: b1,
                },
                View::Array {
                    element: e2,
                    bounds: b2,
                },
            ) => {
                if b1 != b2 {
                    return Assignable::No;
                }
                self.strongly(e1, e2)
            }
            (
                View::Map {
                    key: k1,
                    element: e1,
                    bound: b1,
                },
                View::Map {
                    key: k2,
                    element: e2,
                    bound: b2,
                },
            ) => {
                if !bound_ok(b1, b2, o.ignore_sequence_bounds) {
                    return Assignable::No;
                }
                self.strongly(k1, k2).and(|| self.strongly(e1, e2))
            }
            (View::Enum(e1), View::Enum(e2)) => Assignable::from_bool(enums(e1, e2)),
            (View::Bitmask(b1), View::Bitmask(b2)) => {
                Assignable::from_bool(b1.header.common.bit_bound == b2.header.common.bit_bound)
            }
            (View::Struct(s1), View::Struct(s2)) => self.structs(s1, s2, key_erased),
            (View::Union(u1), View::Union(u2)) => self.unions(u1, u2),
            _ => Assignable::No,
        }
    }

    /// The members of `s`, those of its base types first.
    fn members(&self, s: &'a MinimalStructType) -> Result<Vec<Member<'a>>, Assignable> {
        let mut chain = vec![s];
        let mut base = &s.header.base_type;
        let mut depth = 0;
        while *base != TypeIdentifier::default() {
            depth += 1;
            match self.view(base) {
                Ok(View::Struct(b)) if depth < 64 => {
                    chain.push(b);
                    base = &b.header.base_type;
                }
                Err(missing) => return Err(Assignable::Unknown(vec![missing])),
                _ => return Err(Assignable::No),
            }
        }
        Ok(chain
            .iter()
            .rev()
            .flat_map(|s| &s.member_seq)
            .map(|m| Member {
                id: m.common.member_id,
                flags: m.common.member_flags,
                ty: &m.common.member_type_id,
                name: m.detail.name_hash,
            })
            .collect())
    }

    fn structs(
        &mut self,
        s1: &'a MinimalStructType,
        s2: &'a MinimalStructType,
        key_erased: bool,
    ) -> Assignable {
        let o = self.options;
        let ext = extensibility(s1.struct_flags);
        if ext != extensibility(s2.struct_flags) {
            return Assignable::No;
        }
        let (m1, m2) = match (self.members(s1), self.members(s2)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return e,
        };
        if !o.ignore_member_names && !names_agree(&m1, &m2) {
            return Assignable::No;
        }
        let by_id = |ms: &[Member<'a>], id| ms.iter().position(|m: &Member<'_>| m.id == id);
        // APPENDABLE and FINAL: the members at the same index have the
        // same ID (and FINAL ones have the same members).
        if ext != Extensibility::Mutable && m1.iter().zip(&m2).any(|(a, b)| a.id != b.id) {
            return Assignable::No;
        }
        let mut common = 0;
        let mut result = Assignable::Yes;
        for b in &m2 {
            let Some(i1) = by_id(&m1, b.id) else {
                if o.prevent_type_widening && !b.flags.contains(MemberFlag::IS_OPTIONAL) {
                    return Assignable::No;
                }
                continue;
            };
            common += 1;
            let a = &m1[i1];
            let key = !key_erased
                && (a.flags.contains(MemberFlag::IS_KEY) || b.flags.contains(MemberFlag::IS_KEY));
            if ext != Extensibility::Mutable
                && a.flags.contains(MemberFlag::IS_OPTIONAL)
                    != b.flags.contains(MemberFlag::IS_OPTIONAL)
            {
                return Assignable::No;
            }
            let (t1, t2) = (a.ty, b.ty);
            result = result.and(|| self.assignable(t1, t2, true));
            if ext != Extensibility::Mutable && t1 != t2 {
                result = result.and(|| self.delimited(t2));
            }
            if key {
                let (t1, t2) = (a.ty, b.ty);
                result = result.and(|| self.key_member(t1, t2));
            }
            if result == Assignable::No {
                return Assignable::No;
            }
        }
        if common == 0 {
            return Assignable::No;
        }
        // Members that both must be there.
        let required = |m: &Member<'_>| {
            (!m.flags.contains(MemberFlag::IS_OPTIONAL)
                && m.flags.contains(MemberFlag::IS_MUST_UNDERSTAND))
                || (!key_erased && m.flags.contains(MemberFlag::IS_KEY))
        };
        if m1
            .iter()
            .filter(|m| required(m))
            .any(|m| by_id(&m2, m.id).is_none())
            || m2
                .iter()
                .filter(|m| required(m))
                .any(|m| by_id(&m1, m.id).is_none())
        {
            return Assignable::No;
        }
        if ext == Extensibility::Final && m1.len() != m2.len() {
            return Assignable::No;
        }
        result
    }

    /// The extra rules for a key member (Table 19).
    fn key_member(&mut self, t1: &'a TypeIdentifier, t2: &'a TypeIdentifier) -> Assignable {
        match (self.view(t1), self.view(t2)) {
            (Ok(View::String { bound: b1, .. }), Ok(View::String { bound: b2, .. }))
            | (Ok(View::Sequence { bound: b1, .. }), Ok(View::Sequence { bound: b2, .. }))
            | (Ok(View::Map { bound: b1, .. }), Ok(View::Map { bound: b2, .. })) => {
                Assignable::from_bool(b1 == 0 || (b2 != 0 && b1 >= b2))
            }
            (Ok(View::Enum(e1)), Ok(View::Enum(e2))) => {
                Assignable::from_bool(e2.literal_seq.iter().all(|l2| {
                    e1.literal_seq.iter().any(|l1| {
                        l1.common.value == l2.common.value
                            && l1.detail.name_hash == l2.detail.name_hash
                    })
                }))
            }
            (Ok(View::Struct(_) | View::Union(_)), Ok(View::Struct(_) | View::Union(_))) => {
                self.assignable(t1, t2, false)
            }
            (Err(a), Err(b)) => Assignable::Unknown(vec![a, b]),
            (Err(m), _) | (_, Err(m)) => Assignable::Unknown(vec![m]),
            _ => Assignable::Yes,
        }
    }

    fn unions(&mut self, u1: &'a MinimalUnionType, u2: &'a MinimalUnionType) -> Assignable {
        let o = self.options;
        let ext = extensibility(u1.union_flags);
        if ext != extensibility(u2.union_flags) {
            return Assignable::No;
        }
        let d1 = &u1.discriminator.common;
        let d2 = &u2.discriminator.common;
        if d1.member_flags.contains(MemberFlag::IS_KEY)
            != d2.member_flags.contains(MemberFlag::IS_KEY)
        {
            return Assignable::No;
        }
        if !o.ignore_member_names {
            let as_members = |u: &'a MinimalUnionType| -> Vec<Member<'a>> {
                u.member_seq
                    .iter()
                    .map(|m| Member {
                        id: m.common.member_id,
                        flags: m.common.member_flags,
                        ty: &m.common.type_id,
                        name: m.detail.name_hash,
                    })
                    .collect()
            };
            if !names_agree(&as_members(u1), &as_members(u2)) {
                return Assignable::No;
            }
        }
        let mut result = self.strongly(&d1.type_id, &d2.type_id);
        if result == Assignable::No {
            return result;
        }
        let labels = |u: &'a MinimalUnionType| -> BTreeMap<i32, &'a TypeIdentifier> {
            u.member_seq
                .iter()
                .flat_map(|m| {
                    m.common
                        .label_seq
                        .iter()
                        .map(move |l| (*l, &m.common.type_id))
                })
                .collect()
        };
        let default = |u: &'a MinimalUnionType| {
            u.member_seq
                .iter()
                .find(|m| m.common.member_flags.contains(MemberFlag::IS_DEFAULT))
                .map(|m| &m.common.type_id)
        };
        let (l1, l2) = (labels(u1), labels(u2));
        let (def1, def2) = (default(u1), default(u2));
        let mut pairs: Vec<(&'a TypeIdentifier, &'a TypeIdentifier)> = Vec::new();
        for (label, t2) in &l2 {
            if let Some(t1) = l1.get(label).copied().or(def1) {
                pairs.push((t1, t2));
            }
        }
        if let Some(d2) = def2 {
            for (label, t1) in &l1 {
                if !l2.contains_key(label) {
                    pairs.push((t1, d2));
                }
            }
        }
        if let (Some(a), Some(b)) = (def1, def2) {
            pairs.push((a, b));
        }
        for (t1, t2) in pairs {
            result = result.and(|| self.assignable(t1, t2, false));
            if result == Assignable::No {
                return result;
            }
        }
        let same_labels = l1.keys().eq(l2.keys());
        let shared = l1.keys().any(|l| l2.contains_key(l));
        if ext == Extensibility::Final && !same_labels || ext != Extensibility::Final && !shared {
            return Assignable::No;
        }
        result
    }
}

/// Members with the same name have the same ID, and the other way round.
fn names_agree(m1: &[Member<'_>], m2: &[Member<'_>]) -> bool {
    m1.iter()
        .all(|a| m2.iter().all(|b| (a.id == b.id) == (a.name == b.name)))
}

/// Whether an unsigned integer type of kind `k` holds a bitmask of
/// `bound` bits (Tables 15 and 18).
fn bitmask_holds(k: TypeKind, bound: u16) -> bool {
    match k {
        TK_UINT8 => (1..=8).contains(&bound),
        TK_UINT16 => (9..=16).contains(&bound),
        TK_UINT32 => (17..=32).contains(&bound),
        TK_UINT64 => (33..=64).contains(&bound),
        _ => false,
    }
}

/// Table 18 for two enumerations.
fn enums(e1: &MinimalEnumeratedType, e2: &MinimalEnumeratedType) -> bool {
    let ext = extensibility(e1.enum_flags);
    if ext != extensibility(e2.enum_flags) {
        return false;
    }
    let agree = e1.literal_seq.iter().all(|a| {
        e2.literal_seq.iter().all(|b| {
            (a.detail.name_hash == b.detail.name_hash) == (a.common.value == b.common.value)
        })
    });
    if !agree {
        return false;
    }
    if ext == Extensibility::Final {
        let set = |e: &MinimalEnumeratedType| {
            let mut v: Vec<_> = e
                .literal_seq
                .iter()
                .map(|l| (l.common.value, l.detail.name_hash))
                .collect();
            v.sort();
            v
        };
        return set(e1) == set(e2);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn int32() -> Type {
        Type::Primitive(Primitive::Int32)
    }

    fn st(name: &str, ext: Extensibility, members: Vec<StructMember>) -> Type {
        StructType {
            name: name.into(),
            extensibility: ext,
            members,
            ..Default::default()
        }
        .into()
    }

    fn m(name: &str, id: MemberId, ty: Type) -> StructMember {
        StructMember::new(name, id, ty)
    }

    fn key(mut member: StructMember) -> StructMember {
        member.key = true;
        member
    }

    fn optional(mut member: StructMember) -> StructMember {
        member.optional = true;
        member
    }

    /// Whether `t1` is assignable from `t2`, with both types known.
    fn check(t1: &Type, t2: &Type, options: &TypeConsistency) -> Assignable {
        let (o1, o2) = (TypeObjects::new(t1).unwrap(), TypeObjects::new(t2).unwrap());
        let mut registry = TypeRegistry::new();
        registry.add_type(&o1);
        registry.add_type(&o2);
        is_assignable(o1.minimal_id(), o2.minimal_id(), &registry, options)
    }

    fn yes(t1: &Type, t2: &Type) -> bool {
        check(t1, t2, &TypeConsistency::default()) == Assignable::Yes
    }

    use Extensibility::{Appendable, Final, Mutable};

    #[test]
    fn same_type() {
        let t = st("A", Final, vec![m("x", 0, int32())]);
        assert!(yes(&t, &t));
        // Another name, same members: the minimal TypeObjects are the same.
        let u = st("B", Final, vec![m("x", 0, int32())]);
        assert!(yes(&t, &u));
    }

    #[test]
    fn final_structs_need_the_same_members() {
        let a = st("A", Final, vec![m("x", 0, int32())]);
        let b = st("A", Final, vec![m("x", 0, int32()), m("y", 1, int32())]);
        assert!(!yes(&a, &b));
        assert!(!yes(&b, &a));
        // A member of another type.
        let c = st(
            "A",
            Final,
            vec![m("x", 0, Type::Primitive(Primitive::Int16))],
        );
        assert!(!yes(&a, &c));
    }

    #[test]
    fn appendable_structs_may_add_members_at_the_end() {
        let v1 = st(
            "A",
            Appendable,
            vec![m("x", 0, int32()), m("y", 1, Type::string())],
        );
        let v2 = st(
            "A",
            Appendable,
            vec![
                m("x", 0, int32()),
                m("y", 1, Type::string()),
                m("z", 2, int32()),
            ],
        );
        assert!(yes(&v1, &v2));
        assert!(yes(&v2, &v1));
        // Only with prevent_type_widening is the wider writer refused.
        let strict = TypeConsistency {
            prevent_type_widening: true,
            ..Default::default()
        };
        assert_eq!(check(&v1, &v2, &strict), Assignable::No);
        assert_eq!(check(&v2, &v1, &strict), Assignable::Yes);
        // Not in the middle.
        let v3 = st(
            "A",
            Appendable,
            vec![m("x", 0, int32()), m("z", 2, int32())],
        );
        assert!(!yes(&v1, &v3));
        // Not another extensibility.
        let f = st(
            "A",
            Final,
            vec![m("x", 0, int32()), m("y", 1, Type::string())],
        );
        assert!(!yes(&v1, &f));
    }

    #[test]
    fn mutable_structs_match_by_member_id() {
        let a = st(
            "A",
            Mutable,
            vec![
                m("x", 1, int32()),
                m("y", 2, Type::string()),
                m("z", 3, int32()),
            ],
        );
        let b = st("A", Mutable, vec![m("z", 3, int32()), m("w", 4, int32())]);
        assert!(yes(&a, &b));
        assert!(yes(&b, &a));
        // No member in common.
        let c = st("A", Mutable, vec![m("w", 4, int32())]);
        let d = st("A", Mutable, vec![m("v", 5, int32())]);
        assert!(!yes(&c, &d));
        // The same ID under another name, or the other way round.
        let e = st("A", Mutable, vec![m("x", 1, int32()), m("q", 3, int32())]);
        assert!(!yes(&a, &e));
        let ignore = TypeConsistency {
            ignore_member_names: true,
            ..Default::default()
        };
        assert_eq!(check(&a, &e, &ignore), Assignable::Yes);
        // A member of an incompatible type.
        let f = st("A", Mutable, vec![m("x", 1, Type::string())]);
        assert!(!yes(&a, &f));
    }

    #[test]
    fn keys_and_must_understand_members_are_on_both_sides() {
        let a = st(
            "A",
            Mutable,
            vec![key(m("id", 1, int32())), m("x", 2, int32())],
        );
        let b = st("A", Mutable, vec![m("x", 2, int32())]);
        assert!(!yes(&a, &b));
        assert!(!yes(&b, &a));
        let mut must = m("y", 3, int32());
        must.must_understand = true;
        let c = st("A", Mutable, vec![m("x", 2, int32()), must]);
        assert!(!yes(&b, &c));
        // An optional member need not be there.
        let d = st(
            "A",
            Mutable,
            vec![m("x", 2, int32()), optional(m("y", 3, int32()))],
        );
        assert!(yes(&b, &d));
    }

    #[test]
    fn string_and_sequence_bounds() {
        let s = |b| Type::String {
            wide: false,
            bound: b,
        };
        let seq = |b| {
            Type::Sequence(std::sync::Arc::new(SequenceType {
                element: Element::new(int32()),
                bound: b,
            }))
        };
        let a = |t: Type| st("A", Appendable, vec![m("x", 0, t)]);
        assert!(yes(&a(s(5)), &a(s(10))));
        assert!(yes(&a(seq(5)), &a(seq(0))));
        let strict = TypeConsistency {
            ignore_string_bounds: false,
            ignore_sequence_bounds: false,
            ..Default::default()
        };
        assert_eq!(check(&a(s(5)), &a(s(10)), &strict), Assignable::No);
        assert_eq!(check(&a(s(10)), &a(s(5)), &strict), Assignable::Yes);
        assert_eq!(check(&a(s(0)), &a(s(5)), &strict), Assignable::Yes);
        assert_eq!(check(&a(seq(5)), &a(seq(0)), &strict), Assignable::No);
        // Narrow and wide strings do not mix.
        assert!(!yes(
            &a(s(0)),
            &a(Type::String {
                wide: true,
                bound: 0
            })
        ));
        // Key strings keep their bounds whatever the options.
        let k = |b| st("A", Appendable, vec![key(m("x", 0, s(b)))]);
        assert!(!yes(&k(5), &k(10)));
        assert!(yes(&k(10), &k(5)));
    }

    #[test]
    fn nested_types_must_be_delimited_unless_the_same() {
        let inner_f1 = st("I", Final, vec![m("a", 0, int32())]);
        let inner_f2 = st("I", Final, vec![m("a", 0, int32()), m("b", 1, int32())]);
        let inner_a1 = st("I", Appendable, vec![m("a", 0, int32())]);
        let inner_a2 = st(
            "I",
            Appendable,
            vec![m("a", 0, int32()), m("b", 1, int32())],
        );
        let outer = |i: &Type| {
            st(
                "O",
                Appendable,
                vec![m("i", 0, i.clone()), m("n", 1, int32())],
            )
        };
        assert!(yes(&outer(&inner_f1), &outer(&inner_f1)));
        assert!(!yes(&outer(&inner_f1), &outer(&inner_f2)));
        assert!(yes(&outer(&inner_a1), &outer(&inner_a2)));
        // In a sequence too.
        let seq_of = |i: &Type| st("O", Appendable, vec![m("s", 0, Type::sequence(i.clone()))]);
        assert!(yes(&seq_of(&inner_a1), &seq_of(&inner_a2)));
        // An array keeps its bounds.
        let array = |n| {
            Type::Array(std::sync::Arc::new(ArrayType {
                element: Element::new(int32()),
                dimensions: vec![n],
            }))
        };
        let a = |t| st("A", Final, vec![m("x", 0, t)]);
        assert!(yes(&a(array(3)), &a(array(3))));
        assert!(!yes(&a(array(3)), &a(array(4))));
    }

    #[test]
    fn enums() {
        let e = |ext, lits: &[(&str, i32)]| -> Type {
            EnumType {
                name: "E".into(),
                extensibility: ext,
                literals: lits
                    .iter()
                    .map(|(n, v)| EnumLiteral {
                        name: (*n).into(),
                        value: *v,
                        default: false,
                    })
                    .collect(),
                ..Default::default()
            }
            .into()
        };
        let a = |t: Type| st("A", Appendable, vec![m("x", 0, t)]);
        let f2 = e(Final, &[("A", 0), ("B", 1)]);
        let f3 = e(Final, &[("A", 0), ("B", 1), ("C", 2)]);
        let a2 = e(Appendable, &[("A", 0), ("B", 1)]);
        let a3 = e(Appendable, &[("A", 0), ("B", 1), ("C", 2)]);
        assert!(!yes(&a(f2.clone()), &a(f3)));
        assert!(yes(&a(a2.clone()), &a(a3)));
        assert!(!yes(&a(f2), &a(a2.clone())));
        let renamed = e(Appendable, &[("A", 0), ("X", 1)]);
        assert!(!yes(&a(a2), &a(renamed)));
    }

    #[test]
    fn bitmasks_and_unsigned_integers() {
        let b = |bound| -> Type {
            BitmaskType {
                name: "B".into(),
                bit_bound: bound,
                ..Default::default()
            }
            .into()
        };
        let a = |t: Type| st("A", Final, vec![m("x", 0, t)]);
        assert!(yes(&a(b(16)), &a(b(16))));
        assert!(!yes(&a(b(16)), &a(b(17))));
        assert!(yes(&a(Type::Primitive(Primitive::Uint16)), &a(b(16))));
        assert!(yes(&a(b(32)), &a(Type::Primitive(Primitive::Uint32))));
        assert!(!yes(&a(b(8)), &a(Type::Primitive(Primitive::Uint32))));
    }

    #[test]
    fn aliases_are_resolved() {
        let alias: Type = AliasType {
            name: "L".into(),
            related: int32(),
        }
        .into();
        let a = |t: Type| st("A", Final, vec![m("x", 0, t)]);
        assert!(yes(&a(alias.clone()), &a(int32())));
        assert!(yes(&a(int32()), &a(alias)));
    }

    #[test]
    fn unions() {
        let u = |ext, members: Vec<UnionMember>| -> Type {
            UnionType {
                name: "U".into(),
                extensibility: ext,
                members,
                ..Default::default()
            }
            .into()
        };
        let um = |n: &str, id, t, labels| UnionMember::new(n, id, t, labels);
        let a = |t: Type| st("A", Appendable, vec![m("u", 0, t)]);
        let u1 = u(
            Appendable,
            vec![
                um("a", 1, int32(), vec![1]),
                um("b", 2, Type::string(), vec![2]),
            ],
        );
        let u2 = u(
            Appendable,
            vec![
                um("a", 1, int32(), vec![1]),
                um("b", 2, Type::string(), vec![2]),
                um("c", 3, int32(), vec![3]),
            ],
        );
        assert!(yes(&a(u1.clone()), &a(u2.clone())));
        assert!(yes(&a(u2.clone()), &a(u1.clone())));
        // FINAL unions need the same labels.
        let f1 = u(Final, vec![um("a", 1, int32(), vec![1])]);
        let f2 = u(Final, vec![um("a", 1, int32(), vec![1, 2])]);
        assert!(!yes(&a(f1), &a(f2)));
        // The same label selecting incompatible types.
        let u3 = u(Appendable, vec![um("a", 1, Type::string(), vec![1])]);
        assert!(!yes(&a(u1.clone()), &a(u3)));
        // No label in common.
        let u4 = u(Appendable, vec![um("z", 9, int32(), vec![9])]);
        assert!(!yes(&a(u1), &a(u4)));
    }

    #[test]
    fn unknown_types_are_named() {
        let inner = st("I", Appendable, vec![m("a", 0, int32())]);
        let outer = st("O", Appendable, vec![m("i", 0, inner.clone())]);
        let other = st("O", Appendable, vec![m("i", 0, inner), m("n", 1, int32())]);
        let (o1, o2) = (
            TypeObjects::new(&outer).unwrap(),
            TypeObjects::new(&other).unwrap(),
        );
        let mut registry = TypeRegistry::new();
        registry.add_type(&o1);
        let options = TypeConsistency::default();
        assert_eq!(
            is_assignable(o1.minimal_id(), o2.minimal_id(), &registry, &options),
            Assignable::Unknown(vec![o2.minimal_id().clone()])
        );
        assert_eq!(
            registry.missing(&o2.information),
            vec![o2.minimal_id().clone()]
        );
        registry.add_pairs(&o2.minimal).unwrap();
        assert!(registry.missing(&o2.information).is_empty());
        assert_eq!(
            is_assignable(o1.minimal_id(), o2.minimal_id(), &registry, &options),
            Assignable::Yes
        );
    }

    #[test]
    fn recursive_types() {
        // struct Node { sequence<Node> children; } is not something the
        // builder makes (it refuses cycles), so tie the knot by hand: a
        // struct whose member is a sequence of its own hash.
        let hash = [7; 14];
        let id = TypeIdentifier::Minimal(hash);
        let node = MinimalTypeObject::Struct(MinimalStructType {
            struct_flags: TypeFlag::IS_APPENDABLE,
            header: MinimalStructHeader::default(),
            member_seq: vec![MinimalStructMember {
                common: CommonStructMember {
                    member_id: 0,
                    member_flags: MemberFlag::TRY_CONSTRUCT1,
                    member_type_id: TypeIdentifier::SeqSmall(PlainSequenceSElemDefn {
                        header: PlainCollectionHeader {
                            equiv_kind: EK_MINIMAL,
                            element_flags: MemberFlag::TRY_CONSTRUCT1,
                        },
                        bound: 0,
                        element_identifier: Box::new(id.clone()),
                    }),
                },
                detail: MinimalMemberDetail {
                    name_hash: name_hash("children"),
                },
            }],
        });
        let mut registry = TypeRegistry::new();
        // Not added with add_pairs: the hash is made up.
        registry
            .objects_mut()
            .insert(id.clone(), TypeObject::Minimal(node));
        let other = TypeIdentifier::Minimal([8; 14]);
        let node2 = registry.get(&id).unwrap().clone();
        registry.objects_mut().insert(other.clone(), node2);
        assert_eq!(
            is_assignable(&id, &other, &registry, &TypeConsistency::default()),
            Assignable::Yes
        );
    }

    #[test]
    fn consistency() {
        let v1 = st("A", Appendable, vec![m("x", 0, int32())]);
        let v2 = st(
            "A",
            Appendable,
            vec![m("x", 0, int32()), m("y", 1, int32())],
        );
        let (o1, o2) = (
            TypeObjects::new(&v1).unwrap(),
            TypeObjects::new(&v2).unwrap(),
        );
        let mut registry = TypeRegistry::new();
        registry.add_type(&o1);
        registry.add_type(&o2);
        let ep = |name, info| EndpointType {
            name,
            information: info,
        };
        let allow = TypeConsistency::default();
        let disallow = TypeConsistency {
            kind: TypeConsistencyKind::DisallowTypeCoercion,
            ..Default::default()
        };
        let i1 = Some(&o1.information);
        let i2 = Some(&o2.information);
        use Consistency::*;
        assert_eq!(
            check_consistency(ep("A", i2), ep("A", i1), &allow, &registry),
            Consistent
        );
        // With type information, the names are not compared.
        assert_eq!(
            check_consistency(ep("B", i2), ep("A", i1), &allow, &registry),
            Consistent
        );
        assert_eq!(
            check_consistency(ep("A", i2), ep("A", i1), &disallow, &registry),
            Inconsistent
        );
        assert_eq!(
            check_consistency(ep("A", i1), ep("A", i1), &disallow, &registry),
            Consistent
        );
        // Without, they are, and force_type_validation refuses.
        assert_eq!(
            check_consistency(ep("A", None), ep("A", i1), &allow, &registry),
            Consistent
        );
        assert_eq!(
            check_consistency(ep("B", None), ep("A", i1), &allow, &registry),
            Inconsistent
        );
        let force = TypeConsistency {
            force_type_validation: true,
            ..Default::default()
        };
        assert_eq!(
            check_consistency(ep("A", None), ep("A", i1), &force, &registry),
            Inconsistent
        );
        // A writer type that is not known yet.
        let empty = TypeRegistry::new();
        assert_eq!(
            check_consistency(ep("A", i2), ep("A", i1), &allow, &empty),
            Unknown(vec![o1.minimal_id().clone(), o2.minimal_id().clone()])
        );
    }
}
