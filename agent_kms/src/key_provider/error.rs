use crate::key_provider::aggregate::KeyProviderType;
use cqrs_es::persist::PersistenceError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyProviderError {
    #[error("A `{0}` key provider has already been added")]
    AlreadyAdded(KeyProviderType),
    #[error("No `{0}` key provider has been added")]
    NotFound(KeyProviderType),
    #[error("The `{0}` key provider is only available in the development profile")]
    DevelopmentOnly(KeyProviderType),
    #[error("A key provider name is required")]
    MissingName,
    #[error("Another key provider is already named `{0}`")]
    NameTaken(String),
    #[error("Failed to look up existing key providers: {0}")]
    Lookup(#[from] PersistenceError),
}

impl PartialEq for KeyProviderError {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}
