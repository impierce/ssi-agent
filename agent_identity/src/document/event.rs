use super::aggregate::{IotaMetadata, Status};
use agent_shared::config::SupportedDidMethod;
use cqrs_es::DomainEvent;
use identity_kit::did::{Did, DidDocument};
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum DocumentEvent {
    DocumentCreated {
        document_id: String,
        did_method: SupportedDidMethod,
        status: Status,
        document: DidDocument,
        with_fixed_algorithm: Option<Algorithm>,
        iota_metadata: Option<IotaMetadata>,
    },
    DocumentDidWebOverwritten {
        document_id: String,
        previous_did: Did,
        document: DidDocument,
    },
    PublicKeyUpdated {
        document_id: String,
        document: DidDocument,
    },
    DocumentStatusUpdated {
        document_id: String,
        status: Status,
    },
    ServiceAdded {
        document_id: String,
        document: DidDocument,
    },
    ServiceRemoved {
        document_id: String,
        document: DidDocument,
    },
    DocumentPublished {
        document_id: String,
        document: DidDocument,
        iota_metadata: Option<IotaMetadata>,
    },
    DocumentDeleted {
        document_id: String,
        document: DidDocument,
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
