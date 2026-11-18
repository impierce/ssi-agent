pub mod all_documents;

use super::aggregate::Document;
use cqrs_es::{Aggregate as _, EventEnvelope, View};

pub type DocumentView = Document;
impl View<Document> for Document {
    /// The view is the aggregate itself, so it changes as the aggregate does.
    fn update(&mut self, event: &EventEnvelope<Document>) {
        self.apply(event.payload.clone());
    }
}
