use cqrs_es::DomainEvent;
use oid4vci::{credential_response::CredentialResponse, token_response::TokenResponse};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    OfferCreated {
        pre_authorized_code: String,
        access_token: String,
    },
    CredentialsAdded {
        credential_ids: Vec<String>,
    },
    CredentialOfferCreated {
        form_url_encoded_credential_offer: String,
    },
    TokenResponseCreated {
        token_response: TokenResponse,
    },
    CredentialResponseCreated {
        credential_response: CredentialResponse,
    },
    // PreAuthorizedCodeUpdated {
    //     // subject_id: String,
    //     pre_authorized_code: String,
    // },
    // TokenResponseCreated {
    //     // subject_id: String,
    //     token_response: TokenResponse,
    // },
    // CredentialResponseCreated {
    //     // subject_id: String,
    //     credential_response: CredentialResponse,
    // },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        use OfferEvent::*;

        let event_type: &str = match self {
            OfferCreated { .. } => "OfferCreated",
            CredentialsAdded { .. } => "CredentialAdded",
            CredentialOfferCreated { .. } => "CredentialOfferCreated",
            TokenResponseCreated { .. } => "TokenResponseCreated",
            CredentialResponseCreated { .. } => "CredentialResponseCreated",
            // PreAuthorizedCodeUpdated { .. } => "PreAuthorizedCodeUpdated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
