use super::aggregate::{OfferCredential, Status};
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
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
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
