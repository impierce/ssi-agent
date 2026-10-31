use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("This key has already been recorded")]
    AlreadyRecorded,
}
