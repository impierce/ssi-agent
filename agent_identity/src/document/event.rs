use super::aggregate::Status;
use cqrs_es::DomainEvent;
use identity_document::document::CoreDocument;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: String,
        status: Status,
        document: CoreDocument,
    },
    PublicKeyJwksUpdated {
        document_id: String,
        document: CoreDocument,
    },
    DocumentStatusUpdated {
        document_id: String,
        status: Status,
    },
    ServiceAdded {
        document_id: String,
        document: CoreDocument,
    },
    ServiceRemoved {
        document_id: String,
        document: CoreDocument,
    },
    DocumentPublished {
        document_id: String,
        updated_document: CoreDocument,
    },
}

impl DomainEvent for DocumentEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
