use super::{entity::Data, event::CredentialEvent};
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    pub data: Data,
    pub credential_format_template: serde_json::Value,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        match &event.payload {
            CredentialEvent::UnsignedCredentialCreated {
                data,
                credential_format_template,
            } => {
                self.data = data.clone();
                self.credential_format_template = credential_format_template.clone();
            }
        }
    }
}
