use super::aggregate::Document;
use cqrs_es::{EventEnvelope, View};
use identity_iota::document::CoreDocument;

pub type DocumentView = Document;
impl View<Document> for Document {
    fn update(&mut self, event: &EventEnvelope<Document>) {
        use crate::document::event::DocumentEvent::*;

        match &event.payload {
            DocumentCreated {
                document_id,
                document,
                status,
            } => {
                self.document_id = document_id.clone();
                self.document.replace(document.clone());
                self.status.clone_from(status);
            }
            PublicKeyJwksSet { document, .. } => {
                self.document.replace(document.clone());
            }
            StatusSet { status, .. } => {
                self.status.clone_from(status);
            }
            ServiceAdded { document, .. } => {
                self.document.replace(document.clone());
            }
            ServiceRemoved { document, .. } => {
                self.document.replace(document.clone());
            }
            DocumentPublished {
                document_id,
                updated_document,
            } => {
                self.document_id = document_id.clone();
                // FIX THIS
                self.document.replace(CoreDocument::from(updated_document.clone()));
            }
        }
    }
}
