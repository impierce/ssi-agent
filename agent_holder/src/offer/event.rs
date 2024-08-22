use std::collections::HashMap;

use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferEvent {
    CredentialOfferReceived {
        offer_id: String,
        credential_offer: CredentialOfferParameters,
        credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject>,
    },
    CredentialOfferAccepted {
        offer_id: String,
    },
    TokenResponseReceived {
        offer_id: String,
        token_response: TokenResponse,
    },
    CredentialResponseReceived {
        offer_id: String,
        credentials: Vec<serde_json::Value>,
    },
    CredentialOfferRejected {
        offer_id: String,
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
