use thiserror::Error;

#[derive(Error, Debug)]
pub enum SharedError {
    #[error("Error: {0}")]
    Generic(String),
}
