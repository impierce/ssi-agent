use std::collections::HashMap;

use cqrs_es::DomainEvent;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata,
    credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ServerConfigEvent {
    ServerMetadataInitialized {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    },
    CredentialConfigurationAdded {
        credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject>,
    },
}

impl DomainEvent for ServerConfigEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
