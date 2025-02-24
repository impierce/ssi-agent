pub mod all_documents;

use super::aggregate::Document;
use cqrs_es::{EventEnvelope, View};

pub type DocumentView = Document;
impl View<Document> for Document {
    fn update(&mut self, event: &EventEnvelope<Document>) {
        use crate::document::event::DocumentEvent::*;

        match &event.payload {
            DocumentCreated {
                document_id,
                did_method,
                document,
                status,
                with_fixed_algorithm: signing_algorithm,
            } => {
                self.document_id = document_id.clone();
                self.did_method.replace(did_method.clone());
                self.document.replace(document.clone());
                self.status.clone_from(status);
                self.with_fixed_algorithm.clone_from(signing_algorithm);
            }
            DocumentUpdated {
                document_id,
                document,
                status,
                with_fixed_algorithm: signing_algorithm,
            } => {
                self.document_id = document_id.clone();
                self.document.replace(document.clone());
                self.status.clone_from(status);
                self.with_fixed_algorithm.clone_from(signing_algorithm);
            }
            PublicKeyUpdated { document_id, document } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
            }
            DocumentStatusUpdated { document_id, status } => {
                self.document_id.clone_from(document_id);
                self.status.clone_from(status);
            }
            ServiceAdded { document_id, document } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
            }
            DocumentPublished {
                document_id,
                updated_document,
            } => {
                self.document_id.clone_from(document_id);
                self.document.replace(updated_document.clone());
            }
        }
    }
}
