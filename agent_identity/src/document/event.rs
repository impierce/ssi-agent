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

#[cfg(test)]
mod event_tests {
    use super::super::aggregate::test_utils::document;
    use super::*;

    fn all_variants() -> Vec<DocumentEvent> {
        vec![
            DocumentEvent::DocumentCreated {
                document_id: "document_id".to_string(),
                did_method: SupportedDidMethod::Web,
                status: Status::SignAndValidate,
                document: document(),
                with_fixed_algorithm: Some(Algorithm::EdDSA),
                iota_metadata: Some(IotaMetadata::default()),
            },
            DocumentEvent::PublicKeyUpdated {
                document_id: "document_id".to_string(),
                document: document(),
            },
            DocumentEvent::DocumentStatusUpdated {
                document_id: "document_id".to_string(),
                status: Status::Disabled,
            },
            DocumentEvent::ServiceAdded {
                document_id: "document_id".to_string(),
                document: document(),
            },
            DocumentEvent::DocumentPublished {
                document_id: "document_id".to_string(),
                document: document(),
                iota_metadata: Some(IotaMetadata::default()),
            },
            DocumentEvent::DocumentDeleted {
                document_id: "document_id".to_string(),
                document: document(),
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: DocumentEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    fn document_json() -> serde_json::Value {
        serde_json::json!({
            "id": "did:web:my-domain.example.org",
            "@context": [
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/suites/jws-2020/v1"
            ]
        })
    }

    fn iota_metadata_json() -> serde_json::Value {
        serde_json::json!({
            "wallet_address": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "is_funded": false,
            "balance": 0,
            "is_published": false,
            "is_deactivated": false,
            "explorer_url": null,
            "created_at": null,
            "updated_at": null
        })
    }

    #[test]
    fn golden_document_created() {
        let golden = serde_json::json!({
            "DocumentCreated": {
                "document_id": "document_id",
                "did_method": "did:web",
                "status": "SignAndValidate",
                "document": document_json(),
                "with_fixed_algorithm": "EdDSA",
                "iota_metadata": iota_metadata_json()
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_public_key_updated() {
        let golden = serde_json::json!({
            "PublicKeyUpdated": {
                "document_id": "document_id",
                "document": document_json()
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_document_status_updated() {
        let golden = serde_json::json!({
            "DocumentStatusUpdated": {
                "document_id": "document_id",
                "status": "Disabled"
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_service_added() {
        let golden = serde_json::json!({
            "ServiceAdded": {
                "document_id": "document_id",
                "document": document_json()
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_document_published() {
        let golden = serde_json::json!({
            "DocumentPublished": {
                "document_id": "document_id",
                "document": document_json(),
                "iota_metadata": iota_metadata_json()
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_document_deleted() {
        let golden = serde_json::json!({
            "DocumentDeleted": {
                "document_id": "document_id",
                "document": document_json()
            }
        });

        let event: DocumentEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
