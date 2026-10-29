//! Errors returned by the safe API.

use std::fmt;

use fish_abi::FishStatus;

/// A failed call into the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No host is attached: the call was made outside a Fish mod entrypoint.
    NoHost,
    /// The host rejected the call.
    Host {
        /// What was attempted.
        context: &'static str,
        /// The status the host returned.
        status: FishStatus,
    },
    /// The host table or a service is too old for the requested function.
    Unsupported(&'static str),
    /// An argument was invalid (for example a malformed identifier).
    InvalidArgument(String),
}

impl Error {
    pub(crate) fn host(context: &'static str, status: FishStatus) -> Error {
        Error::Host { context, status }
    }

    /// The host status behind this error, if any.
    pub fn status(&self) -> Option<FishStatus> {
        match self {
            Error::Host { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoHost => {
                f.write_str("no Fish host is attached (called outside a mod entrypoint)")
            }
            Error::Host { context, status } => write!(f, "{context} failed: {}", status.name()),
            Error::Unsupported(what) => write!(f, "the host does not support {what}"),
            Error::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Turns a host status into a `Result`.
pub(crate) fn check(context: &'static str, status: FishStatus) -> Result<(), Error> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(Error::host(context, status))
    }
}
