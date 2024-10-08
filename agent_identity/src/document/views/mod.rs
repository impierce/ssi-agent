use super::aggregate::Document;
use cqrs_es::{EventEnvelope, View};

pub type DocumentView = Document;
impl View<Document> for Document {
    fn update(&mut self, event: &EventEnvelope<Document>) {
        use crate::document::event::DocumentEvent::*;

        match &event.payload {
            DocumentCreated { document, .. } => {
                self.document.replace(document.clone());
            }
            ServiceAdded { document, .. } => {
                self.document.replace(document.clone());
            }
        }
    }
}
