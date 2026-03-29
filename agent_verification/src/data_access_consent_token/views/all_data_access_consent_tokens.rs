use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::data_access_consent_token::{aggregate::DataAccessConsentToken, views::DataAccessConsentTokenView};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllDataAccessConsentTokensView {
    #[serde(flatten)]
    pub data_access_consent_tokens: HashMap<String, DataAccessConsentTokenView>,
}

impl View<DataAccessConsentToken> for AllDataAccessConsentTokensView {
    fn update(&mut self, event: &EventEnvelope<DataAccessConsentToken>) {
        self.data_access_consent_tokens
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
