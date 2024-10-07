use cqrs_es::DomainEvent;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};

use super::aggregate::Data;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum CredentialEvent {
    CredentialAdded {
        credential_id: String,
        offer_id: String,
        credential: Jwt,
        data: Data,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        use CredentialEvent::*;

        let event_type: &str = match self {
            CredentialAdded { .. } => "CredentialAdded",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
