use super::aggregate::{OfferCredential, Status};
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    CredentialOfferReceived {
        received_offer_id: String,
        credential_offer: Box<CredentialOfferParameters>,
        credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject>,
    },
    CredentialOfferAccepted {
        received_offer_id: String,
        status: Status,
    },
    TokenResponseReceived {
        received_offer_id: String,
        token_response: TokenResponse,
    },
    CredentialResponseReceived {
        received_offer_id: String,
        status: Status,
        credentials: Vec<OfferCredential>,
    },
    CredentialOfferRejected {
        received_offer_id: String,
        status: Status,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        use OfferEvent::*;

        let event_type: &str = match self {
            CredentialOfferReceived { .. } => "CredentialOfferReceived",
            CredentialOfferAccepted { .. } => "CredentialOfferAccepted",
            TokenResponseReceived { .. } => "AccessTokenReceived",
            CredentialResponseReceived { .. } => "CredentialResponseReceived",
            CredentialOfferRejected { .. } => "CredentialOfferRejected",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
