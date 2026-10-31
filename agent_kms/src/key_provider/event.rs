use crate::key_provider::aggregate::KeyProviderType;
use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Display)]
pub enum KeyProviderEvent {
    KeyProviderAdded {
        provider_type: KeyProviderType,
        name: String,
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
