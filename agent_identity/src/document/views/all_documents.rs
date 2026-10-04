use super::DocumentView;
use crate::document::aggregate::Document;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllDocumentsView {
    #[serde(flatten)]
    pub documents: IndexMap<String, DocumentView>,
}

impl View<Document> for AllDocumentsView {
    fn update(&mut self, event: &EventEnvelope<Document>) {
        self.documents
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
