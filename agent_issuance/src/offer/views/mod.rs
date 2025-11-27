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
                grant_types,
                status,
                credential_offer,
                credential_offer_uri,
                pre_authorized_code,
                tx_code,
            } => {
                self.offer_id.clone_from(offer_id);
                self.grant_types.clone_from(grant_types);
                self.status.clone_from(status);
                self.credential_offer.replace(credential_offer.clone());
                self.credential_offer_uri.replace(credential_offer_uri.clone());
                self.pre_authorized_code.clone_from(pre_authorized_code);
                self.tx_code.clone_from(tx_code);
            }
            CredentialsAdded {
                offer_id,
                credential_ids,
                credential_offer,
            } => {
                self.offer_id.clone_from(offer_id);
                self.credential_ids.clone_from(credential_ids);
                self.credential_offer.replace(credential_offer.clone());
            }
            FormUrlEncodedCredentialOfferCreated {
                offer_id,
                form_url_encoded_credential_offer,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.form_url_encoded_credential_offer
                    .replace(form_url_encoded_credential_offer.clone());
                self.status.clone_from(status);
            }
            CredentialOfferSent {
                offer_id,
                target_url: _target_url,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.status.clone_from(status);
            }
            CredentialRequestVerified { offer_id, subject_id } => {
                self.offer_id.clone_from(offer_id);
                self.subject_id.clone_from(subject_id);
            }
            CredentialResponseCreated {
                offer_id,
                credential_response,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.credential_response.replace(credential_response.clone());
                self.status.clone_from(status);
            }
        }
    }
}
