use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::credential::aggregate::Credential;

pub struct SimpleLoggingQuery {}

#[async_trait]
impl Query<Credential> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Credential>]) {}
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CredentialView {
    aggregate_id: Option<String>,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        println!("TODO: should be updating the view ...");
    }
}
