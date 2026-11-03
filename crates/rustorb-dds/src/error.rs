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
    /// The participant was deleted.
    AlreadyDeleted,
    /// No more entity ids are left in the participant.
    OutOfResources,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "network error: {e}"),
            Error::Serialize(e) => write!(f, "cannot serialize the sample: {e}"),
            Error::AlreadyDeleted => f.write_str("the participant was deleted"),
            Error::OutOfResources => f.write_str("no more entity ids"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Serialize(e) => Some(e),
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

/// The result of a DCPS operation.
pub type Result<T> = std::result::Result<T, Error>;
