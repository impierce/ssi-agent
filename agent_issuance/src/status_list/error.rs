use thiserror::Error;

#[derive(Error, Debug)]
pub enum StatusListError {
    #[error("Invalid Status List URL (equal to the Status List ID): {0}")]
    InvalidURL(String),
}
