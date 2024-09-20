pub mod all_offers;

use super::aggregate::Status;
use crate::offer::aggregate::Offer;
use cqrs_es::{EventEnvelope, View};
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct OfferView {
    pub credential_offer: Option<CredentialOfferParameters>,
    pub status: Status,
    pub credential_configurations: Option<HashMap<String, CredentialConfigurationsSupportedObject>>,
    pub token_response: Option<TokenResponse>,
    pub credentials: Vec<serde_json::Value>,
}

impl View<Offer> for OfferView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            CredentialOfferReceived {
                credential_offer,
                credential_configurations,
                ..
            } => {
                self.credential_offer.replace(*credential_offer.clone());
                self.credential_configurations
                    .replace(credential_configurations.clone());
            }
            CredentialOfferAccepted { status, .. } => {
                self.status.clone_from(status);
            }
            TokenResponseReceived { token_response, .. } => {
                self.token_response.replace(token_response.clone());
            }
            CredentialResponseReceived {
                status, credentials, ..
            } => {
                self.status.clone_from(status);
                self.credentials.clone_from(credentials);
            }
            CredentialOfferRejected { status, .. } => {
                self.status.clone_from(status);
            }
        }
    }
}
