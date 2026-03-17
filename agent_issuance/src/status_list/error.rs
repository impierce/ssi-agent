use thiserror::Error;

#[derive(Error, Debug)]
pub enum StatusListError {
    #[error("Invalid Status List URL (equal to the Status List ID): {0}")]
    InvalidURL(String),
    #[error("Failed to set index `{0}` due to error: {1}")]
    FailedToSetIndex(usize, String),
}
