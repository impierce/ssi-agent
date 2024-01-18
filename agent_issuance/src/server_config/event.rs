use cqrs_es::DomainEvent;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
    credentials_supported::CredentialsSupportedObject,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ServerConfigEvent {
    AuthorizationServerMetadataLoaded {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    },
    CredentialIssuerMetadataLoaded {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CredentialsSupportedCreated {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
}

impl DomainEvent for ServerConfigEvent {
    fn event_type(&self) -> String {
        use ServerConfigEvent::*;

        let event_type: &str = match self {
            AuthorizationServerMetadataLoaded { .. } => "AuthorizationServerMetadataLoaded",
            CredentialIssuerMetadataLoaded { .. } => "CredentialIssuerMetadataLoaded",
            CredentialsSupportedCreated { .. } => "CredentialsSupportedCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
