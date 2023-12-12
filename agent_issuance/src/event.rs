use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    },
    credential_response::CredentialResponse,
    token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};

use crate::model::aggregate::{Credential, CredentialOffer, Image, IssuanceSubject};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IssuanceEvent {
    ImageUploaded {
        image: Image,
    },
    CredentialFormatTemplateLoaded {
        credential_format_template: serde_json::Value,
    },
    AuthorizationServerMetadataLoaded {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    },
    CredentialIssuerMetadataLoaded {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    SubjectCreated {
        subject: IssuanceSubject,
    },
    CredentialsSupportedCreated {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
    UnsignedCredentialCreated {
        subject_id: String,
        credential: Credential,
    },
    PreAuthorizedCodeUpdated {
        subject_id: String,
        pre_authorized_code: String,
    },
    CredentialOfferCreated {
        subject_id: String,
        credential_offer: CredentialOffer,
    },
    TokenResponseCreated {
        subject_id: String,
        token_response: TokenResponse,
    },
    CredentialResponseCreated {
        subject_id: String,
        credential_response: CredentialResponse,
    },
}

impl DomainEvent for IssuanceEvent {
    fn event_type(&self) -> String {
        use IssuanceEvent::*;

        let event_type: &str = match self {
            ImageUploaded { .. } => "ImageUploaded",
            CredentialFormatTemplateLoaded { .. } => "CredentialFormatTemplateLoaded",
            AuthorizationServerMetadataLoaded { .. } => "AuthorizationServerMetadataLoaded",
            CredentialIssuerMetadataLoaded { .. } => "CredentialIssuerMetadataLoaded",
            CredentialsSupportedCreated { .. } => "CredentialsSupportedCreated",
            SubjectCreated { .. } => "SubjectCreated",
            CredentialOfferCreated { .. } => "CredentialOfferCreated",
            UnsignedCredentialCreated { .. } => "UnsignedCredentialCreated",
            PreAuthorizedCodeUpdated { .. } => "PreAuthorizedCodeUpdated",
            TokenResponseCreated { .. } => "TokenResponseCreated",
            CredentialResponseCreated { .. } => "CredentialResponseCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
