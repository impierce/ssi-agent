use crate::key_provider::{aggregate::KeyProvider, views::all_key_providers::AllKeyProvidersView};
use cqrs_es::persist::PersistenceError;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::{Arc, OnceLock};

pub type AllKeyProvidersRepository = Arc<dyn DynViewRepository<AllKeyProvidersView, KeyProvider>>;

pub struct KeyProviderServices {
    /// Whether development-only providers, such as the in-memory provider, may be added.
    pub development_providers_allowed: bool,
    /// Set once the view repositories exist, which is only after the command handler that needs these services.
    pub(crate) all_key_providers: OnceLock<AllKeyProvidersRepository>,
}

impl KeyProviderServices {
    #[must_use]
    pub fn new(development_providers_allowed: bool) -> Self {
        Self {
            development_providers_allowed,
            all_key_providers: OnceLock::new(),
        }
    }

    /// Whether another provider already uses `name`. Provider names are unique across all provider types.
    ///
    /// # Errors
    ///
    /// Fails when the existing providers can't be loaded.
    pub async fn is_name_taken(&self, name: &str) -> Result<bool, PersistenceError> {
        let Some(all_key_providers) = self.all_key_providers.get() else {
            return Ok(false);
        };

        Ok(all_key_providers
            .load(AllKeyProvidersView::VIEW_ID)
            .await?
            .is_some_and(|view| view.key_providers.values().any(|provider| provider.name == name)))
    }
}
