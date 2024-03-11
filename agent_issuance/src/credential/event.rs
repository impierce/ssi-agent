use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use super::entity::Data;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum CredentialEvent {
    UnsignedCredentialCreated {
        data: Data,
        credential_format_template: serde_json::Value,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        use CredentialEvent::*;

        let event_type: &str = match self {
            UnsignedCredentialCreated { .. } => "UnsignedCredentialCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
