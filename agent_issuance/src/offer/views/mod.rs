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
                pre_authorized_code,
                access_token,
                status,
                credential_offer,
                credential_offer_uri,
                tx_code,
                delivery_options,
            } => {
                self.offer_id.clone_from(offer_id);
                self.grant_types.clone_from(grant_types);
                self.pre_authorized_code.clone_from(pre_authorized_code);
                self.access_token.clone_from(access_token);
                self.status.clone_from(status);
                self.credential_offer.replace(credential_offer.clone());
                self.credential_offer_uri.replace(credential_offer_uri.clone());
                self.tx_code.clone_from(tx_code);
                self.delivery_options.clone_from(delivery_options);
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
            CredentialOfferEmailSent {
                offer_id,
                recipient_email: _recipient_email,
                form_url_encoded_credential_offer: _form_url_encoded_credential_offer,
                status,
            } => {
                self.offer_id.clone_from(offer_id);
                self.status.clone_from(status);
            }
            CredentialRequestVerified { offer_id, subject_id } => {
                self.offer_id.clone_from(offer_id);
                self.subject_id.replace(subject_id.clone());
            }
            TokenResponseCreated {
                offer_id,
                token_response,
            } => {
                self.offer_id.clone_from(offer_id);
                self.token_response.replace(token_response.clone());
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
            TxCodeGenerated {
                offer_id,
                tx_code,
                delivery_options,
            } => {
                self.offer_id.clone_from(offer_id);
                self.tx_code.replace(tx_code.clone());
                self.delivery_options.clone_from(delivery_options);
            }
        }
    }
}
