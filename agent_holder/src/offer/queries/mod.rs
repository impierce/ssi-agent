pub mod all_offers;

use std::collections::HashMap;

use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    EventEnvelope, Query, View,
};
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};

use crate::offer::aggregate::Offer;

use super::aggregate::Status;

/// A custom query trait for the Offer aggregate. This trait is used to define custom queries for the Offer aggregate
/// that do not make use of `GenericQuery`.
#[async_trait]
pub trait CustomQuery<R, V>: Query<Offer>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError>;

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<Offer>]) -> Result<(), PersistenceError>;
}

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
