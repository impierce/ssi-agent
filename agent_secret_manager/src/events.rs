use cqrs_es::DomainEvent;
use did_manager::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SecretManagerEvent {
    Initialized,
    DidMethodEnabled { method: Method },
}

impl DomainEvent for SecretManagerEvent {
    fn event_type(&self) -> String {
        match self {
            SecretManagerEvent::Initialized => "Initialized".to_string(),
            SecretManagerEvent::DidMethodEnabled { .. } => "DidMethodEnabled".to_string(),
        }
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
