use super::HolderCredentialView;
use crate::credential::queries::Credential;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllHolderCredentialsView {
    #[serde(flatten)]
    pub credentials: IndexMap<String, HolderCredentialView>,
}

impl View<Credential> for AllHolderCredentialsView {
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
