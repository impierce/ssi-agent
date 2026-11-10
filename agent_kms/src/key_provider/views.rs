use crate::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    event::KeyProviderEvent,
};
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The key providers this instance is connected to, in the order they were added.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyProvidersView {
    pub providers: IndexMap<KeyProviderType, String>,
}

impl View<KeyProviders> for KeyProvidersView {
    fn update(&mut self, event: &EventEnvelope<KeyProviders>) {
        match &event.payload {
            KeyProviderEvent::KeyProviderAdded { provider_type, name } => {
                self.providers.insert(*provider_type, name.clone());
            }
            KeyProviderEvent::KeyProviderRemoved { provider_type } => {
                self.providers.shift_remove(provider_type);
            }
        }
    }
}
