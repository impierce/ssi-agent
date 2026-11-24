use crate::key_provider::aggregate::KeyProviderType;
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
    /// The provider type needs settings, and none were given.
    #[error("A `{0}` key provider needs its settings")]
    MissingSettings(KeyProviderType),
    /// The settings are for another provider type, or the provider type takes none.
    #[error("These settings are not for a `{0}` key provider")]
    WrongSettings(KeyProviderType),
}

impl PartialEq for KeyProviderError {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}
