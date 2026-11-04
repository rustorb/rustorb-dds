//! The DDS-XTypes 1.3 type system for rustorb-dds.
//!
//! - [`type_object`]: TypeIdentifier, TypeObject and TypeInformation
//!   (Annex B, 7.3.4, 7.6.3.2.1), their XCDR2 serialization and the hashes
//!   that identify types.
//! - [`types`]: a description of a type (7.2.2, the type system model)
//!   and the TypeObjects and TypeInformation built from it.
//!
//! - [`registry`]: the TypeObjects a participant knows.
//! - [`assignability`]: whether one type is assignable from another
//!   (7.2.4), and the type consistency enforcement of endpoint matching
//!   (7.6.3.4).
//!
//! - [`typelookup`]: the requests and replies of the TypeLookup service
//!   (7.6.3.3), with which participants ask each other for TypeObjects.
//!
//! - [`dynamic`]: the Dynamic Language Binding (7.5.2): DynamicType and
//!   DynamicData, for types known only at run time, and their XCDR1 and
//!   XCDR2 serialization.
//!
//! The XCDR1/XCDR2 encodings themselves are in rustorb-cdr, shared with
//! rustorb.

pub mod assignability;
pub mod dynamic;
pub mod registry;
pub mod type_object;
pub mod typelookup;
pub mod types;
pub mod xcdr2;

use std::fmt;

/// Errors of this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The CDR encoder or decoder failed.
    Cdr(rustorb_cdr::Error),
    /// Serialized data that is not a valid value of its type.
    Invalid(&'static str),
    /// A type description that cannot be represented, with the reason.
    InvalidType(String),
    /// Something this version does not do yet.
    Unsupported(&'static str),
    /// A bad argument to an operation of the Dynamic Language Binding
    /// (`RETCODE_BAD_PARAMETER`), with the reason.
    BadParameter(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Cdr(e) => write!(f, "CDR: {e}"),
            Error::Invalid(what) => write!(f, "invalid data: {what}"),
            Error::InvalidType(what) => write!(f, "invalid type: {what}"),
            Error::Unsupported(what) => write!(f, "not supported: {what}"),
            Error::BadParameter(what) => write!(f, "bad parameter: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Cdr(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rustorb_cdr::Error> for Error {
    fn from(e: rustorb_cdr::Error) -> Error {
        Error::Cdr(e)
    }
}

/// The result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
