use thiserror::Error;

#[derive(Error, Debug)]
pub enum SharedError {
    #[error("Error while loading `{0}`: {1}")]
    GenericConfigurationError(String, String),
    // This error should always be unreachable since all configuration fields should have a default value in development mode
    #[error("Configuration parameter `{0}` is missing a default value")]
    MissingDefaultValueForDevelopment(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Configuration is not suitable for production: {0}")]
    ConfigurationNotSuitableForProduction(String),
}
