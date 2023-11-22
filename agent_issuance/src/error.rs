use std::fmt::{Display, Formatter};

// TODO: Use thiserror crate.
#[derive(Debug)]
pub struct IssuanceError(String);

impl Display for IssuanceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for IssuanceError {}

impl From<&str> for IssuanceError {
    fn from(message: &str) -> Self {
        IssuanceError(message.to_string())
    }
}
