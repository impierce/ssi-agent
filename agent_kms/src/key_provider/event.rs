use crate::key_provider::{aggregate::KeyProviderType, settings::KeyProviderSettings};
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display)]
pub enum KeyProviderEvent {
    KeyProviderAdded {
        provider_type: KeyProviderType,
        name: String,
        /// Absent for provider types without settings, and in events recorded before providers had any.
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
    use crate::key_provider::settings::{StackitKmsSettings, StackitRegion};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn reads_a_provider_added_before_providers_had_settings() {
        let event: KeyProviderEvent = serde_json::from_value(json!({
            "KeyProviderAdded": { "provider_type": "aws_kms_ambient", "name": "Production keys" }
        }))
        .unwrap();

        assert_eq!(
            event,
            KeyProviderEvent::KeyProviderAdded {
                provider_type: KeyProviderType::AwsKmsAmbient,
                name: "Production keys".to_string(),
                settings: None,
            }
        );
    }

    #[test]
    fn records_the_settings_of_a_provider() {
        let event = KeyProviderEvent::KeyProviderAdded {
            provider_type: KeyProviderType::StackitKms,
            name: "Own keys".to_string(),
            settings: Some(KeyProviderSettings::StackitKms(StackitKmsSettings {
                project_id: Uuid::from_u128(1),
                region: StackitRegion::Eu01,
                key_ring_id: Uuid::from_u128(2),
                service_account_email: "unicore@sa.stackit.cloud".to_string(),
            })),
        };

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "KeyProviderAdded": {
                    "provider_type": "stackit_kms",
                    "name": "Own keys",
                    "settings": {
                        "stackit_kms": {
                            "project_id": "00000000-0000-0000-0000-000000000001",
                            "region": "eu01",
                            "key_ring_id": "00000000-0000-0000-0000-000000000002",
                            "service_account_email": "unicore@sa.stackit.cloud"
                        }
                    }
                }
            })
        );
    }
}
