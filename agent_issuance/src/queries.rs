use async_trait::async_trait;
use cqrs_es::{persist::GenericQuery, EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::model::aggregate::IssuanceData;

pub struct SimpleLoggingQuery {}

// Our simplest query, this is great for debugging but absolutely useless in production.
// This query just pretty prints the events as they are processed.
#[async_trait]
impl Query<IssuanceData> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<IssuanceData>]) {
        for event in events {
            let payload = serde_json::to_string_pretty(&event.payload).unwrap();
            println!("{}-{}\n{}", aggregate_id, event.sequence, payload);
        }
    }
}

pub type CredentialQuery<R> = GenericQuery<R, IssuanceDataView, IssuanceData>;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct IssuanceDataView {
    credential_template: serde_json::Value,
    credential_data: serde_json::Value,
}

impl View<IssuanceData> for IssuanceDataView {
    fn update(&mut self, event: &EventEnvelope<IssuanceData>) {
        use crate::event::IssuanceEvent::*;

        match &event.payload {
            AuthorizationServerMetadataLoaded { .. } => todo!(),
            CredentialIssuerMetadataLoaded { .. } => todo!(),
            CredentialsSupportedCreated { .. } => todo!(),
            CredentialOfferCreated { .. } => todo!(),
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
