use crate::document::aggregate::IotaMetadata;

use super::aggregate::Status;
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

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
