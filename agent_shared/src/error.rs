use thiserror::Error;

#[derive(Error, Debug)]
pub enum SharedError {
    #[error("Error: {0}")]
    Generic(String),
    #[error("Configuration is not suitable for production: {0}")]
    ConfigurationNotSuitableForProduction(String),
}
