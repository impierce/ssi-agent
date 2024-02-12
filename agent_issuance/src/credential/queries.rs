use async_trait::async_trait;
use cqrs_es::{Aggregate, EventEnvelope, Query, View};
use oid4vci::credential;
use serde::{Deserialize, Serialize};

use crate::credential::aggregate::Credential;

use super::{entity::Data, event::CredentialEvent, value_object::Subject};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    // // Entity
    // pub data: Data,
    // // Value Objects
    pub credential_format_template: serde_json::Value,
    // pub subject: Subject,
    pub credential: serde_json::Value,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        match &event.payload {
            CredentialEvent::CredentialFormatTemplateLoaded {
                credential_format_template,
            } => {
                self.credential_format_template = credential_format_template.clone();
            }
            CredentialEvent::UnsignedCredentialCreated { credential } => {
                // self.data = credential.data.clone();
                // // self.subject = credential.subject.clone();
                // self.credential_format_template = credential.credential_format_template.clone();
                self.credential = credential.clone();
            }
            _ => {}
        }
    }
}
