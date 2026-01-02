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
                iota_metadata,
            } => {
                self.document_id = document_id.clone();
                self.did_method.replace(*did_method);
                self.document.replace(document.clone());
                self.status.clone_from(status);
                self.with_fixed_algorithm.clone_from(signing_algorithm);
                self.iota_metadata.clone_from(iota_metadata);
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
            VerificationMethodAdded {
                document_id,
                document,
                verification_method_ids,
            } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
                self.verification_method_ids.clone_from(verification_method_ids);
            }
            VerificationMethodRemoved {
                document_id,
                document,
                verification_method_ids,
            } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
                self.verification_method_ids.clone_from(verification_method_ids);
            }
            DocumentPublished {
                document_id,
                document,
                iota_metadata,
            } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
                self.iota_metadata.clone_from(iota_metadata);
            }
            DocumentDeleted { document_id, document } => {
                self.document_id.clone_from(document_id);
                self.document.replace(document.clone());
            }
        }
    }
}
