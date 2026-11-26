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
    /// A key that isn't a configured issuer key, such as the key the Key Management System names `key_id`, became
    /// the verification method `verification_method_id` of `document`.
    PublicKeyAdded {
        document_id: String,
        verification_method_id: DidUrl,
        key_id: String,
        document: DidDocument,
    },
    /// The verification method of an added key left `document`.
    PublicKeyRemoved {
        document_id: String,
        verification_method_id: DidUrl,
        document: DidDocument,
    },
    /// `verification_method_id` signs for `algorithm` from now on.
    SigningKeySelected {
        document_id: String,
        algorithm: Algorithm,
        verification_method_id: DidUrl,
    },
    /// The configured issuer key signs for `algorithm` again.
    SigningKeyDeselected {
        document_id: String,
        algorithm: Algorithm,
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
