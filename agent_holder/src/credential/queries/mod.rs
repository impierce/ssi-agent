pub mod all_credentials;

use super::event::CredentialEvent;
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct HolderCredentialView {
    pub credential_id: Option<String>,
    pub offer_id: Option<String>,
    pub credential: Option<serde_json::Value>,
}

impl View<Credential> for HolderCredentialView {
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
                self.credential.replace(credential.clone());
            }
        }
    }
}
