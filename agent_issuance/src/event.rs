// use crate::model::aggregate::CredentialTemplate;
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    },
    credential_offer::CredentialOffer,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IssuanceEvent {
    AuthorizationServerMetadataLoaded {
        authorization_server_metadata: AuthorizationServerMetadata,
    },
    CredentialIssuerMetadataLoaded {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CredentialsSupportedCreated {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
    CredentialTemplateLoaded {
        credential_template: serde_json::Value,
    },
    CredentialOfferCreated {
        credential_offer: CredentialOffer,
    },
    CredentialDataCreated {
        credential_template: serde_json::Value,
        credential_data: serde_json::Value,
    },
    CredentialSigned,
}

impl DomainEvent for IssuanceEvent {
    fn event_type(&self) -> String {
        use IssuanceEvent::*;

        let event_type: &str = match self {
            AuthorizationServerMetadataLoaded { .. } => "AuthorizationServerMetadataLoaded",
            CredentialIssuerMetadataLoaded { .. } => "CredentialIssuerMetadataLoaded",
            CredentialsSupportedCreated { .. } => "CredentialsSupportedObjectsCreated",
            CredentialOfferCreated { .. } => "CredentialOfferCreated",
            CredentialTemplateLoaded { .. } => "CredentialTemplateCreated",
            CredentialDataCreated { .. } => "CredentialDataCreated",
            CredentialSigned { .. } => "CredentialSigned",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
