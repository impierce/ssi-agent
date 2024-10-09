pub mod all_offers;

use crate::offer::aggregate::Offer;
use cqrs_es::{EventEnvelope, View};

pub type OfferView = Offer;

impl View<Offer> for Offer {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        match &event.payload {
            CredentialOfferCreated {
                offer_id,
                pre_authorized_code,
                access_token,
                ..
            } => {
                self.offer_id.clone_from(offer_id);
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
