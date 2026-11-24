use crate::key_provider::{aggregate::KeyProviderType, settings::KeyProviderSettings};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display)]
pub enum KeyProviderEvent {
    KeyProviderAdded {
        provider_type: KeyProviderType,
        name: String,
        /// `None` for a provider type without settings, and for providers added before providers had settings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settings: Option<KeyProviderSettings>,
    },
    KeyProviderRemoved {
        provider_type: KeyProviderType,
    },
}

impl DomainEvent for KeyProviderEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_provider::settings::tests::stackit;
    use serde_json::json;

    #[test]
    fn records_the_settings_of_a_provider() {
        let event = KeyProviderEvent::KeyProviderAdded {
            provider_type: KeyProviderType::StackitKms,
            name: "Customer keys".to_string(),
            settings: Some(stackit("key-ring")),
        };

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "KeyProviderAdded": {
                    "provider_type": "stackit_kms",
                    "name": "Customer keys",
                    "settings": {
                        "projectId": "4a1d2f6e-0f33-4c55-9b56-6f8e8a2b1c3d",
                        "region": "eu01",
                        "keyRingId": "key-ring",
                        "serviceAccountEmail": "unicore-kms@sa.stackit.cloud"
                    }
                }
            })
        );
    }

    #[test]
    fn reads_a_provider_added_before_providers_had_settings() {
        let stored = json!({ "KeyProviderAdded": { "provider_type": "aws_kms_ambient", "name": "Production keys" } });

        let event: KeyProviderEvent = serde_json::from_value(stored).unwrap();

        assert_eq!(
            event,
            KeyProviderEvent::KeyProviderAdded {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "Production keys".to_string(),
                settings: None,
            }
        );
    }
}
