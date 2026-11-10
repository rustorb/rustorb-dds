//! The Dynamic Language Binding of DDS-XTypes 1.3 (clause 7.5.2): types
//! and values handled at run time, for types the program was not built
//! with.
//!
//! - [`DynamicType`], [`TypeDescriptor`], [`MemberDescriptor`],
//!   [`DynamicTypeMember`]: a type, introspected. A `DynamicType` wraps a
//!   [`Type`](crate::types::Type) of the type system model.
//! - [`DynamicTypeBuilderFactory`] and [`DynamicTypeBuilder`]: types built
//!   at run time, copied, or made from TypeObjects
//!   ([`DynamicTypeBuilderFactory::create_type_w_type_object`]).
//! - [`DynamicDataFactory`] and [`DynamicData`]: values of those types,
//!   with getters and setters by member ID, and their XCDR1 and XCDR2
//!   serialization (7.4.3, [`DynamicData::to_payload`],
//!   [`DynamicData::from_payload`]).
//! - [`ContentFilter`]: the filter expressions of content filtered
//!   topics (DDS 1.4, Annex B), evaluated on DynamicData.
//!
//! The API follows the names of the specification, in Rust: operations
//! return [`Result`](crate::Result) instead of a `ReturnCode_t`, the
//! factories have associated functions instead of singletons, and loans
//! are borrows ([`DynamicData::loan_value`]).
//!
//! Annotation and bitset types are not supported, nor types that depend
//! on each other (as in [`TypeObjects`](crate::types::TypeObjects)).

mod cdr;
mod convert;
mod data;
pub mod filter;
mod ty;

pub use data::{DynamicData, DynamicDataFactory};
pub use filter::ContentFilter;
pub use ty::{
    DynamicType, DynamicTypeBuilder, DynamicTypeBuilderFactory, DynamicTypeMember,
    MemberDescriptor, TypeDescriptor,
};

use crate::type_object::MemberId;

/// `MEMBER_ID_INVALID`: names the value of a DynamicData itself, rather
/// than one of its members (7.5.2.11.1).
pub const MEMBER_ID_INVALID: MemberId = 0x0fff_ffff;
