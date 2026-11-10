//! The builtin plugins of DDS Security 1.2 for rustorb-dds: the
//! cryptographic plugin DDS:Crypto:AES-GCM-GMAC ([`crypto`]), with the
//! authentication (DDS:Auth:PKI-DH) and access control
//! (DDS:Access:Permissions) plugins to come.
//!
//! Written from the OMG specification (DDS Security 1.2, formal/25-03-06)
//! with RustCrypto.

pub mod access;
pub mod auth;
pub mod cdr;
pub mod crypto;
pub mod pki;
pub mod smime;
pub mod token;
pub mod xml;

/// A failure of a security operation (a SecurityException).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    /// An error saying `m`.
    pub fn new(m: impl Into<String>) -> Self {
        Error(m.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
