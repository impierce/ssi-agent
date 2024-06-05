pub mod access_token;
pub mod pre_authorized_code;

use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    EventEnvelope, Query, View,
};
use oid4vci::{credential_response::CredentialResponse, token_response::TokenResponse};
use serde::{Deserialize, Serialize};

use crate::offer::aggregate::Offer;

use super::event::OfferEvent;

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
    pub subject_id: Option<String>,
    pub credential_ids: Vec<String>,
    pub pre_authorized_code: String,
    pub access_token: String,
    pub form_url_encoded_credential_offer: String,
    pub token_response: Option<TokenResponse>,
    pub credential_response: Option<CredentialResponse>,
}

impl View<Offer> for OfferView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            CredentialOfferCreated {
                pre_authorized_code,
                access_token,
                ..
            } => {
                self.pre_authorized_code.clone_from(pre_authorized_code);
                self.access_token.clone_from(access_token)
            }
            CredentialsAdded {
                credential_ids: credential_id,
                ..
            } => {
                self.credential_ids.clone_from(credential_id);
            }
            FormUrlEncodedCredentialOfferCreated {
                form_url_encoded_credential_offer,
                ..
            } => self
                .form_url_encoded_credential_offer
                .clone_from(form_url_encoded_credential_offer),
            CredentialRequestVerified { subject_id, .. } => {
                self.subject_id.replace(subject_id.clone());
            }
            TokenResponseCreated { token_response, .. } => {
                self.token_response.replace(token_response.clone());
            }
            CredentialResponseCreated {
                credential_response, ..
            } => {
                self.credential_response.replace(credential_response.clone());
            }
        }
    }
}
