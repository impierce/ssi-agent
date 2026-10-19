use crate::key_provider::{aggregate::KeyProvider, views::KeyProviderView};
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllKeyProvidersView {
    #[serde(flatten)]
    pub key_providers: IndexMap<String, KeyProviderView>,
}

impl AllKeyProvidersView {
    pub const VIEW_ID: &'static str = "all_key_providers";
}

impl View<KeyProvider> for AllKeyProvidersView {
    fn update(&mut self, event: &EventEnvelope<KeyProvider>) {
        let view = self.key_providers.entry(event.aggregate_id.clone()).or_default();
        view.update(event);
        if view.removed {
            self.key_providers.shift_remove(&event.aggregate_id);
        }
    }
}
