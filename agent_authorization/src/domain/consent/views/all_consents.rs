use super::Consent;
use super::ConsentView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllConsentsView {
    #[serde(flatten)]
    pub consents: HashMap<String, ConsentView>,
}

impl View<Consent> for AllConsentsView {
    fn update(&mut self, event: &EventEnvelope<Consent>) {
        self.consents
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
