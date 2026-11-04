//! Errors of the DCPS API.

use std::fmt;

/// Why a DCPS operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The network could not be set up or a datagram could not be sent.
    Io(std::io::Error),
    /// A sample could not be serialized.
    Serialize(rustorb_typesupport::Error),
    /// A DynamicData sample could not be serialized, or a DynamicType
    /// cannot be used for a topic.
    Dynamic(rustorb_xtypes::Error),
    /// The participant was deleted.
    AlreadyDeleted,
    /// No more entity ids are left in the participant.
    OutOfResources,
    /// The operation did not finish in time: a reliable writer had no
    /// room in its history within `max_blocking_time`, or its readers
    /// did not acknowledge in time.
    Timeout,
    /// The operation is not allowed in this state: unregistering an
    /// instance that is not registered.
    PreconditionNotMet,
    /// The QoS asks for something not implemented: TRANSIENT or
    /// PERSISTENT durability.
    Unsupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "network error: {e}"),
            Error::Serialize(e) => write!(f, "cannot serialize the sample: {e}"),
            Error::Dynamic(e) => write!(f, "dynamic type or data: {e}"),
            Error::AlreadyDeleted => f.write_str("the participant was deleted"),
            Error::OutOfResources => f.write_str("no more entity ids"),
            Error::Timeout => f.write_str("timed out"),
            Error::PreconditionNotMet => f.write_str("the instance is not registered"),
            Error::Unsupported(what) => write!(f, "not supported: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Serialize(e) => Some(e),
            Error::Dynamic(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rustorb_typesupport::Error> for Error {
    fn from(e: rustorb_typesupport::Error) -> Self {
        Error::Serialize(e)
    }
}

impl From<rustorb_xtypes::Error> for Error {
    fn from(e: rustorb_xtypes::Error) -> Self {
        Error::Dynamic(e)
    }
}

/// The result of a DCPS operation.
pub type Result<T> = std::result::Result<T, Error>;
