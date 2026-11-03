//! Errors of the message codec.

use std::fmt;

use crate::types::ProtocolVersion;

/// Why a message, a submessage or a parameter list could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The input ended before a field could be read.
    UnexpectedEnd,
    /// The message does not start with `RTPS` (DDSI-RTPS 2.5, 8.3.6.3).
    NotRtps([u8; 4]),
    /// The major protocol version is newer than this implementation
    /// (DDSI-RTPS 2.5, 8.3.6.3).
    UnsupportedVersion(ProtocolVersion),
    /// A submessage header could not be read in full, or its
    /// `octetsToNextHeader` goes past the end of the message
    /// (DDSI-RTPS 2.5, 8.3.4.1 rules 1 and 2).
    BadSubmessageLength {
        /// Offset of the submessage header from the start of the message.
        offset: usize,
    },
    /// A known submessage is invalid (DDSI-RTPS 2.5, 8.3.4.1 rule 6 and
    /// the "Validity" part of each submessage in 8.3.8).
    InvalidSubmessage {
        /// The submessage id.
        id: u8,
        /// Which rule was broken.
        reason: &'static str,
    },
    /// A parameter list is malformed (DDSI-RTPS 2.5, 9.4.2.11).
    InvalidParameterList(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnexpectedEnd => f.write_str("unexpected end of input"),
            DecodeError::NotRtps(p) => write!(f, "not an RTPS message (protocol {p:02x?})"),
            DecodeError::UnsupportedVersion(v) => {
                write!(f, "unsupported RTPS version {}.{}", v.major, v.minor)
            }
            DecodeError::BadSubmessageLength { offset } => {
                write!(f, "bad submessage length at offset {offset}")
            }
            DecodeError::InvalidSubmessage { id, reason } => {
                write!(f, "invalid submessage 0x{id:02x}: {reason}")
            }
            DecodeError::InvalidParameterList(reason) => {
                write!(f, "invalid parameter list: {reason}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Why a message could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeError {
    /// A submessage that is not the last one in the message is longer
    /// than 65535 octets, so `octetsToNextHeader` cannot hold its length
    /// (DDSI-RTPS 2.5, 9.4.5.1.3).
    SubmessageTooLong {
        /// Index of the submessage in the message.
        index: usize,
    },
    /// A submessage other than PAD or INFO_TS has an empty body and is not
    /// the last one in the message. Its `octetsToNextHeader` would be 0,
    /// which means "up to the end of the message" (DDSI-RTPS 2.5,
    /// 9.4.5.1.3).
    EmptySubmessage {
        /// Index of the submessage in the message.
        index: usize,
    },
    /// A HeaderExtension is not the first submessage
    /// (DDSI-RTPS 2.5, 8.3.7.3).
    MisplacedHeaderExtension {
        /// Index of the submessage in the message.
        index: usize,
    },
    /// A parameter value is longer than a parameter length can say.
    ParameterTooLong,
    /// A locator list has more entries than a 32-bit count can say.
    TooManyLocators,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::SubmessageTooLong { index } => {
                write!(f, "submessage {index} is too long and is not the last one")
            }
            EncodeError::EmptySubmessage { index } => {
                write!(f, "submessage {index} is empty and is not the last one")
            }
            EncodeError::MisplacedHeaderExtension { index } => {
                write!(
                    f,
                    "HeaderExtension at index {index}; it must be the first submessage"
                )
            }
            EncodeError::ParameterTooLong => f.write_str("parameter value is too long"),
            EncodeError::TooManyLocators => f.write_str("too many locators"),
        }
    }
}

impl std::error::Error for EncodeError {}
