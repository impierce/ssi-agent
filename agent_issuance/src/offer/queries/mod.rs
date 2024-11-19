pub mod access_token;
pub mod all_offers;
pub mod pre_authorized_code;

use super::event::OfferEvent;
use crate::offer::aggregate::Offer;
use cqrs_es::{persist::ViewRepository, EventEnvelope, View};
use oid4vci::{
    credential_offer::CredentialOffer, credential_response::CredentialResponse, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct OfferView {
    pub credential_offer: Option<CredentialOffer>,
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
            CredentialOfferSent { .. } => {}
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
