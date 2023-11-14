use crate::model::aggregate::CredentialTemplate;
use cqrs_es::DomainEvent;
use identity_credential::credential::Credential;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IssuanceEvent {
    CredentialDataCreated {
        credential_template: CredentialTemplate,
        credential_data: serde_json::Value,
    },
    CredentialSigned,
}

impl DomainEvent for IssuanceEvent {
    fn event_type(&self) -> String {
        use IssuanceEvent::*;

        let event_type: &str = match self {
            CredentialDataCreated { .. } => "CredentialDataCreated",
            CredentialSigned { .. } => "CredentialSigned",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
