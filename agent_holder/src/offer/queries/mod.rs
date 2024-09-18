pub mod all_offers;

use crate::offer::aggregate::Offer;
use cqrs_es::{EventEnvelope, View};

pub type OfferView = Offer;

impl View<Offer> for Offer {
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
