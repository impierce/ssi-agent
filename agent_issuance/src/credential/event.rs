use cqrs_es::DomainEvent;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use serde::{Deserialize, Serialize};

use super::entity::Data;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum CredentialEvent {
    // TODO: rename to `DataCredentialCreated`?
    UnsignedCredentialCreated {
        data: Data,
        credential_configuration: CredentialConfigurationsSupportedObject,
    },
    SignedCredentialCreated {
        signed_credential: serde_json::Value,
    },
    CredentialSigned {
        signed_credential: serde_json::Value,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        use CredentialEvent::*;

        let event_type: &str = match self {
            UnsignedCredentialCreated { .. } => "UnsignedCredentialCreated",
            SignedCredentialCreated { .. } => "SignedCredentialCreated",
            CredentialSigned { .. } => "CredentialSigned",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
