use async_trait::async_trait;
use cqrs_es::{persist::GenericQuery, EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::model::aggregate::Credential;

pub struct SimpleLoggingQuery {}

// Our simplest query, this is great for debugging but absolutely useless in production.
// This query just pretty prints the events as they are processed.
#[async_trait]
impl Query<Credential> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Credential>]) {
        for event in events {
            let payload = serde_json::to_string_pretty(&event.payload).unwrap();
            println!("{}-{}\n{}", aggregate_id, event.sequence, payload);
        }
    }
}

pub type CredentialQuery<R> = GenericQuery<R, CredentialView, Credential>;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    credential_template: serde_json::Value,
    credential_data: serde_json::Value,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        use crate::event::IssuanceEvent::*;

        match &event.payload {
            CredentialTemplateLoaded { credential_template } => {
                self.credential_template = credential_template.clone();
            }
            CredentialDataCreated {
                credential_template,
                credential_data,
            } => {
                self.credential_template = credential_template.clone();
                self.credential_data = credential_data.clone();
            }
            CredentialSigned => todo!(),
        }
    }
}
