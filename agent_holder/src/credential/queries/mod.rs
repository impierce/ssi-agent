pub mod all_credentials;

use super::event::CredentialEvent;
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};

pub type CredentialView = Credential;

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        use CredentialEvent::*;

        match &event.payload {
            CredentialAdded {
                credential_id,
                offer_id,
                credential,
            } => {
                self.credential_id.replace(credential_id.clone());
                self.offer_id.replace(offer_id.clone());
                self.signed.replace(credential.clone());
            }
        }
    }
}
