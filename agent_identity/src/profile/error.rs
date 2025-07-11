use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("The resource cannot be modified at runtime because it was provisioned by a static configuration file.")]
    ConfigurationConflict,
}
