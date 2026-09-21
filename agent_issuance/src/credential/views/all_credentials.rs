use super::CredentialView;
use crate::credential::views::Credential;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllCredentialsView {
    #[serde(flatten)]
    pub credentials: IndexMap<String, CredentialView>,
}

impl View<Credential> for AllCredentialsView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        self.credentials
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
