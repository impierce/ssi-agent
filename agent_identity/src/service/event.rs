use cqrs_es::DomainEvent;
use derivative::Derivative;
use identity_document::service::Service as DocumentService;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::aggregate::ServiceResource;

#[derive(Clone, Debug, Deserialize, Serialize, Derivative, Display)]
#[derivative(PartialEq)]
pub enum ServiceEvent {
    DomainLinkageServiceCreated {
        service_id: String,
        service: DocumentService,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
        is_deleted: bool,
    },
    DomainLinkageServiceDeleted {
        service_id: String,
        service: Option<DocumentService>,
        #[derivative(PartialEq = "ignore")]
        resource: Option<ServiceResource>,
        is_deleted: bool,
    },
    LinkedVerifiablePresentationServiceCreated {
        service_id: String,
        presentation_ids: Vec<String>,
        service: DocumentService,
    },
}

impl DomainEvent for ServiceEvent {
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
    use super::super::aggregate::test_utils::{
        domain_linkage_resource, domain_linkage_service, linked_verifiable_presentation_service,
    };
    use super::*;

    fn service_json() -> serde_json::Value {
        serde_json::json!({
            "id": "did:place:holder#service-1",
            "type": "LinkedDomains",
            "serviceEndpoint": {
                "origins": ["https://my-domain.example.org/"]
            }
        })
    }

    fn resource_json() -> serde_json::Value {
        serde_json::json!({
            "DomainLinkage": {
                "@context": "https://identity.foundation/.well-known/did-configuration/v1",
                "linked_dids": [
                    "eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDp3ZWI6bXktZG9tYWluLmV4YW1wbGUub3JnI2tleS0wIn0.eyJleHAiOjMxNTM2MDAwLCJpc3MiOiJkaWQ6d2ViOm15LWRvbWFpbi5leGFtcGxlLm9yZyIsIm5iZiI6MCwic3ViIjoiZGlkOndlYjpteS1kb21haW4uZXhhbXBsZS5vcmciLCJ2YyI6eyJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsImh0dHBzOi8vaWRlbnRpdHkuZm91bmRhdGlvbi8ud2VsbC1rbm93bi9kaWQtY29uZmlndXJhdGlvbi92MSJdLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiRG9tYWluTGlua2FnZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsib3JpZ2luIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvIn19fQ.l7dEPioa-No5zBlDCthfXDcffRB7371OnLrrQQgeAdnvHhs5F8XqRtdAWKXB8z3Se00WtGxHrTepLKmH9OWJDQ"
                ]
            }
        })
    }

    fn all_variants() -> Vec<ServiceEvent> {
        vec![
            ServiceEvent::DomainLinkageServiceCreated {
                service_id: "service-1".to_string(),
                service: domain_linkage_service("service-1".to_string()),
                resource: domain_linkage_resource(),
                is_deleted: false,
            },
            ServiceEvent::DomainLinkageServiceDeleted {
                service_id: "service-1".to_string(),
                service: None,
                resource: None,
                is_deleted: true,
            },
            ServiceEvent::LinkedVerifiablePresentationServiceCreated {
                service_id: "service-1".to_string(),
                presentation_ids: vec!["presentation-1".to_string()],
                service: linked_verifiable_presentation_service("service-1".to_string()),
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        // `ServiceEvent`'s `PartialEq` (via `derivative`) intentionally ignores the `resource`
        // field, so we compare the re-serialized JSON values instead to also lock that field's
        // wire format.
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: ServiceEvent = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&deserialized).unwrap(), value);
        }
    }

    #[test]
    fn golden_domain_linkage_service_created() {
        let golden = serde_json::json!({
            "DomainLinkageServiceCreated": {
                "service_id": "service-1",
                "service": service_json(),
                "resource": resource_json(),
                "is_deleted": false
            }
        });

        let event: ServiceEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_domain_linkage_service_deleted() {
        let golden = serde_json::json!({
            "DomainLinkageServiceDeleted": {
                "service_id": "service-1",
                "service": null,
                "resource": null,
                "is_deleted": true
            }
        });

        let event: ServiceEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_linked_verifiable_presentation_service_created() {
        let golden = serde_json::json!({
            "LinkedVerifiablePresentationServiceCreated": {
                "service_id": "service-1",
                "presentation_ids": ["presentation-1"],
                "service": {
                    "id": "did:place:holder#service-1",
                    "type": "LinkedVerifiablePresentation",
                    "serviceEndpoint": ["https://my-domain.example.org/linked-verifiable-presentations/presentation-1"]
                }
            }
        });

        let event: ServiceEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
