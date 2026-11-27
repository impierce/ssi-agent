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
    pub providers: IndexMap<KeyProviderType, String>,
    /// The settings of every provider in `providers` that has any. Views stored before providers had settings have
    /// none.
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
    use crate::key_provider::settings::tests::stackit;
    use std::collections::HashMap;

    fn envelope(payload: KeyProviderEvent) -> EventEnvelope<KeyProviders> {
        EventEnvelope {
            aggregate_id: KeyProviders::ID.to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn forgets_the_settings_of_a_removed_provider() {
        let mut view = KeyProvidersView::default();
        view.update(&envelope(KeyProviderEvent::KeyProviderAdded {
            provider_type: KeyProviderType::StackitKms,
            name: "Customer keys".to_string(),
            settings: Some(stackit("key-ring")),
        }));

        view.update(&envelope(KeyProviderEvent::KeyProviderRemoved {
            provider_type: KeyProviderType::StackitKms,
        }));

        assert!(view.settings.is_empty());
    }

    #[test]
    fn reads_a_view_stored_before_providers_had_settings() {
        let stored = serde_json::json!({ "providers": { "aws_kms_ambient": "Production keys" } });

        let view: KeyProvidersView = serde_json::from_value(stored).unwrap();

        assert_eq!(
            view.providers,
            IndexMap::from([(KeyProviderType::AwsKmsAmbient, "Production keys".to_string())])
        );
    }
}
