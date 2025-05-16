use super::PresentationView;
use crate::presentation::aggregate::Presentation;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllPresentationsView {
    #[serde(flatten)]
    pub presentations: HashMap<String, PresentationView>,
}

impl View<Presentation> for AllPresentationsView {
    fn update(&mut self, event: &EventEnvelope<Presentation>) {
        self.presentations
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
