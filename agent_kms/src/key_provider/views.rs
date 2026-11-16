use crate::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    event::KeyProviderEvent,
    settings::KeyProviderSettings,
};
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The key providers this instance is connected to, in the order they were added.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyProvidersView {
    /// Provider names by type.
    pub providers: IndexMap<KeyProviderType, String>,
    /// The settings of the providers that have any, by type. Kept apart from the names so that views stored before
    /// providers had settings can still be read.
    #[serde(default)]
    pub settings: IndexMap<KeyProviderType, KeyProviderSettings>,
}

impl View<KeyProviders> for KeyProvidersView {
    fn update(&mut self, event: &EventEnvelope<KeyProviders>) {
        match &event.payload {
            KeyProviderEvent::KeyProviderAdded {
                provider_type,
                name,
                settings,
                ..
            } => {
                self.providers.insert(*provider_type, name.clone());
                if let Some(settings) = settings {
                    self.settings.insert(*provider_type, settings.clone());
                }
            }
            KeyProviderEvent::KeyProviderRemoved { provider_type } => {
                self.providers.shift_remove(provider_type);
                self.settings.shift_remove(provider_type);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_provider::settings::{StackitKmsSettings, StackitRegion};
    use serde_json::json;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn envelope(payload: KeyProviderEvent) -> EventEnvelope<KeyProviders> {
        EventEnvelope {
            aggregate_id: KeyProviders::ID.to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn reads_a_view_stored_before_providers_had_settings() {
        let view: KeyProvidersView =
            serde_json::from_value(json!({ "providers": { "aws_kms_ambient": "Production keys" } })).unwrap();

        assert!(view.settings.is_empty());
    }

    #[test]
    fn forgets_the_settings_of_a_removed_provider() {
        let mut view = KeyProvidersView::default();
        view.update(&envelope(KeyProviderEvent::KeyProviderAdded {
            provider_type: KeyProviderType::StackitKms,
            name: "Own keys".to_string(),
            settings: Some(KeyProviderSettings::StackitKms(StackitKmsSettings {
                project_id: Uuid::from_u128(1),
                region: StackitRegion::Eu01,
                key_ring_id: Uuid::from_u128(2),
                service_account_email: "unicore@sa.stackit.cloud".to_string(),
            })),
        }));

        view.update(&envelope(KeyProviderEvent::KeyProviderRemoved {
            provider_type: KeyProviderType::StackitKms,
        }));

        assert!(view.settings.is_empty());
    }
}
