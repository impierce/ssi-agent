use super::aggregate::Status;
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_offer::CredentialOffer, credential_response::CredentialResponse, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    CredentialOfferCreated {
        offer_id: String,
        credential_offer: CredentialOffer,
        pre_authorized_code: String,
        access_token: String,
    },
    CredentialsAdded {
        offer_id: String,
        credential_ids: Vec<String>,
    },
    FormUrlEncodedCredentialOfferCreated {
        offer_id: String,
        form_url_encoded_credential_offer: String,
        status: Status,
    },
    CredentialOfferSent {
        offer_id: String,
        target_url: Url,
        status: Status,
    },
    TokenResponseCreated {
        offer_id: String,
        token_response: TokenResponse,
    },
    CredentialRequestVerified {
        offer_id: String,
        subject_id: String,
    },
    CredentialResponseCreated {
        offer_id: String,
        credential_response: CredentialResponse,
        status: Status,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        use OfferEvent::*;

        let event_type: &str = match self {
            CredentialOfferCreated { .. } => "CredentialOfferCreated",
            CredentialsAdded { .. } => "CredentialsAdded",
            FormUrlEncodedCredentialOfferCreated { .. } => "FormUrlEncodedCredentialOfferCreated",
            CredentialOfferSent { .. } => "CredentialOfferSent",
            TokenResponseCreated { .. } => "TokenResponseCreated",
            CredentialRequestVerified { .. } => "CredentialRequestVerified",
            CredentialResponseCreated { .. } => "CredentialResponseCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
