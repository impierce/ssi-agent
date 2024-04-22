use super::{entity::Data, event::CredentialEvent};
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    pub data: Option<Data>,
    pub credential_format_template: Option<serde_json::Value>,
    pub signed: Option<serde_json::Value>,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        match &event.payload {
            CredentialEvent::UnsignedCredentialCreated {
                data,
                credential_format_template,
            } => {
                self.data = Some(data.clone());
                self.credential_format_template = Some(credential_format_template.clone());
            }
            CredentialEvent::SignedCredentialCreated { signed_credential } => {
                self.signed = Some(signed_credential.clone());
            }
            CredentialEvent::CredentialSigned { signed_credential } => {
                self.signed = Some(signed_credential.clone());
            }
        }
    }
}
