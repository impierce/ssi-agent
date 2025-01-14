use cqrs_es::DomainEvent;
use identity_document::document::CoreDocument;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: String,
        document: CoreDocument,
    },
    ServiceAdded {
        document: CoreDocument,
    },
    DocumentPublished {
        document_id: String,
    },
}

impl DomainEvent for DocumentEvent {
    fn event_type(&self) -> String {
        use DocumentEvent::*;

        let event_type: &str = match self {
            DocumentCreated { .. } => "DocumentCreated",
            ServiceAdded { .. } => "ServiceAdded",
            DocumentPublished { .. } => "DocumentPublished",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
