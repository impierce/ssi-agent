use super::aggregate::{IotaMetadata, Status};
use agent_shared::config::SupportedDidMethod;
use cqrs_es::DomainEvent;
use identity_document::document::CoreDocument;
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: String,
        did_method: SupportedDidMethod,
        status: Status,
        document: CoreDocument,
        with_fixed_algorithm: Option<Algorithm>,
        iota_metadata: Option<IotaMetadata>,
    },
    PublicKeyUpdated {
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
    DocumentPublished {
        document_id: String,
        document: CoreDocument,
        iota_metadata: Option<IotaMetadata>,
    },
    DocumentDeleted {
        document_id: String,
        document: CoreDocument,
    },
}

impl DomainEvent for DocumentEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
