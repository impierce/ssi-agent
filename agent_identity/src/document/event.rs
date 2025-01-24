use cqrs_es::DomainEvent;
use identity_document::document::CoreDocument;
use serde::{Deserialize, Serialize};

use super::aggregate::Status;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: String,
        status: Status,
        document: CoreDocument,
    },
    PublicKeyJwkAdded {
        document_id: String,
        document: CoreDocument,
    },
    StatusSet {
        document_id: String,
        status: Status,
    },
    ServiceAdded {
        document: CoreDocument,
    },
    ServiceRemoved {
        document: CoreDocument,
    },
    DocumentPublished {
        document_id: String,
        updated_document: CoreDocument,
    },
}

impl DomainEvent for DocumentEvent {
    fn event_type(&self) -> String {
        use DocumentEvent::*;

        let event_type: &str = match self {
            DocumentCreated { .. } => "DocumentCreated",
            PublicKeyJwkAdded { .. } => "PublicKeyJwkAdded",
            StatusSet { .. } => "StatusSet",
            ServiceAdded { .. } => "ServiceAdded",
            ServiceRemoved { .. } => "ServiceRemoved",
            DocumentPublished { .. } => "DocumentPublished",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
