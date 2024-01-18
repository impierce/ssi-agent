use cqrs_es::DomainEvent;
use oid4vci::{
    credential_offer::CredentialOffer, credential_response::CredentialResponse, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    PreAuthorizedCodeUpdated {
        // subject_id: String,
        pre_authorized_code: String,
    },
    CredentialOfferCreated {
        // subject_id: String,
        credential_offer: CredentialOffer,
    },
    TokenResponseCreated {
        // subject_id: String,
        token_response: TokenResponse,
    },
    CredentialResponseCreated {
        // subject_id: String,
        credential_response: CredentialResponse,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        use OfferEvent::*;

        let event_type: &str = match self {
            PreAuthorizedCodeUpdated { .. } => "PreAuthorizedCodeUpdated",
            CredentialOfferCreated { .. } => "CredentialOfferCreated",
            TokenResponseCreated { .. } => "TokenResponseCreated",
            CredentialResponseCreated { .. } => "CredentialResponseCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
