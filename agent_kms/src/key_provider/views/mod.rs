pub mod all_key_providers;

use crate::key_provider::{
    aggregate::{KeyProvider, KeyProviderType},
    event::KeyProviderEvent,
};
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use shared_kernel::view_repository::SoftDeletable;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyProviderView {
    pub provider_type: Option<KeyProviderType>,
    pub name: String,
    pub removed: bool,
}

impl View<KeyProvider> for KeyProviderView {
    fn update(&mut self, event: &EventEnvelope<KeyProvider>) {
        match &event.payload {
            KeyProviderEvent::KeyProviderAdded { provider_type, name } => {
                *self = Self {
                    provider_type: Some(*provider_type),
                    name: name.clone(),
                    removed: false,
                };
            }
            KeyProviderEvent::KeyProviderRemoved { .. } => self.removed = true,
        }
    }
}

impl SoftDeletable for KeyProviderView {
    fn is_deleted(&self) -> bool {
        self.removed
    }
}
