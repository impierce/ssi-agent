use async_trait::async_trait;
use cqrs_es::{persist::GenericQuery, EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::model::aggregate::{IssuanceData, IssuanceSubject, OID4VCIData};

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
    pub credential_format_template: serde_json::Value,
    pub oid4vci_data: OID4VCIData,
    pub subjects: Vec<IssuanceSubject>,
}

impl View<IssuanceData> for IssuanceDataView {
    fn update(&mut self, event: &EventEnvelope<IssuanceData>) {
        use crate::event::IssuanceEvent::*;

        match &event.payload {
            CredentialFormatTemplateLoaded {
                credential_format_template,
            } => {
                self.credential_format_template = credential_format_template.clone();
            }
            AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            } => {
                self.oid4vci_data
                    .authorization_server_metadata
                    .replace(*authorization_server_metadata.clone());
            }
            CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            } => {
                self.oid4vci_data.credential_issuer_metadata = Some(credential_issuer_metadata.clone());
            }
            CredentialsSupportedCreated { credentials_supported } => {
                self.oid4vci_data
                    .credential_issuer_metadata
                    .as_mut()
                    .unwrap()
                    .credentials_supported = credentials_supported.clone();
            }
            SubjectCreated { subject } => {
                self.subjects.push(subject.clone());
            }
            CredentialOfferCreated {
                subject_id,
                credential_offer,
            } => {
                self.subjects
                    .iter_mut()
                    .find(|s| s.id == *subject_id)
                    .unwrap()
                    .credential_offer
                    .replace(credential_offer.clone());
            }
            UnsignedCredentialCreated { credential } => {
                self.subjects[0].credentials.push(credential.clone());
            }
            TokenResponseCreated { token_response } => {
                self.subjects[0].token_response.replace(token_response.clone());
            }
            CredentialResponseCreated { credential_response } => {
                self.subjects[0]
                    .credential_response
                    .replace(credential_response.clone());
            }
        }
    }
}
