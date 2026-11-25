use thiserror::Error;

/// Why a [`RefreshCapability`](crate::refresh::aggregate::RefreshCapability) refused a command.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RefreshCapabilityError {
    /// A capability was already created for this token.
    #[error("The refresh capability already exists")]
    AlreadyExists,
    /// The capability was never created.
    #[error("The refresh capability does not exist")]
    NotFound,
    /// A replacement of the capability was accepted, so it prepares no further continuations.
    #[error("The refresh capability is retired")]
    Retired,
    /// Another refresh recorded a continuation after the one the caller saw.
    #[error("Another continuation was recorded first")]
    ContinuationSuperseded,
}
