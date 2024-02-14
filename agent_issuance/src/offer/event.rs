use cqrs_es::DomainEvent;
use oid4vci::{credential_response::CredentialResponse, token_response::TokenResponse};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    CredentialOfferCreated {
        pre_authorized_code: String,
        access_token: String,
    },
    CredentialsAdded {
        credential_ids: Vec<String>,
    },
    FormUrlEncodedCredentialOfferCreated {
        form_url_encoded_credential_offer: String,
    },
    TokenResponseCreated {
        token_response: TokenResponse,
    },
    CredentialResponseCreated {
        credential_response: CredentialResponse,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        use OfferEvent::*;

        let event_type: &str = match self {
            CredentialOfferCreated { .. } => "OfferCreated",
            CredentialsAdded { .. } => "CredentialAdded",
            FormUrlEncodedCredentialOfferCreated { .. } => "CredentialOfferCreated",
            TokenResponseCreated { .. } => "TokenResponseCreated",
            CredentialResponseCreated { .. } => "CredentialResponseCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
