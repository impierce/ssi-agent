use thiserror::Error;

/// Why the `Key` aggregate refused a command.
#[derive(Debug, Error, PartialEq)]
pub enum KeyError {
    /// The key is recorded already, or was recorded and deleted. A key is recorded only once.
    #[error("The key is already recorded")]
    AlreadyRecorded,
    /// The key was never recorded, or has been deleted.
    #[error("There is no such key")]
    NotFound,
}
