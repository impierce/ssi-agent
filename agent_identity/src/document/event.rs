use super::aggregate::Status;
use agent_shared::config::SupportedDidMethod;
use cqrs_es::DomainEvent;
use identity_kit::did::{Did, DidDocument, DidUrl};
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
    PublicKeyAdded {
        document_id: String,
        document: DidDocument,
        verification_method_id: DidUrl,
        /// The ID of the key in the Key Management System. Absent for keys added before it was recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_id: Option<String>,
    },
    PublicKeyRemoved {
        document_id: String,
        document: DidDocument,
        verification_method_id: DidUrl,
    },
    SigningKeySelected {
        document_id: String,
        algorithm: Algorithm,
        verification_method_id: DidUrl,
    },
    SigningKeyDeselected {
        document_id: String,
        algorithm: Algorithm,
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
