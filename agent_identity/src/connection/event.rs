use crate::connection::aggregate::{ConnectionDisplayProperties, PendingChanges, Validation};
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ConnectionEvent {
    ConnectionAdded {
        connection_id: String,
        display: Option<ConnectionDisplayProperties>,
        url: Url,
        dids: Vec<DIDUrl>,
        first_interacted_at: Option<DateTime<Utc>>,
        last_interacted_at: Option<DateTime<Utc>>,
        validations: Vec<Validation>,
    },
    ConnectionRemoved {
        connection_id: String,
    },
    ConnectionSynced {
        connection_id: String,
        validations: Vec<Validation>,
        pending_changes: Option<PendingChanges>,
        last_interacted_at: Option<DateTime<Utc>>,
    },
    ConnectionChangesAccepted {
        connection_id: String,
        display: Option<ConnectionDisplayProperties>,
        dids: Vec<DIDUrl>,
        last_interacted_at: Option<DateTime<Utc>>,
        pending_changes: Option<PendingChanges>,
    },
}

impl DomainEvent for ConnectionEvent {
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
    use super::*;
    use crate::connection::aggregate::{DomainLinkageValidation, LogoProperties, ValidationResult};

    const TEST_DID: &str = "did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM";

    fn fixed_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1700000000, 0).unwrap()
    }

    fn display() -> ConnectionDisplayProperties {
        ConnectionDisplayProperties {
            name: Some("Time Regulation Institute".to_string()),
            locale: Some("en".to_string()),
            logo: Some(LogoProperties {
                uri: Some("https://example.com/logo.png".parse().unwrap()),
                alt_text: Some("Organisational Logo".to_string()),
            }),
        }
    }

    fn validations() -> Vec<Validation> {
        vec![Validation::DomainLinkage(DomainLinkageValidation {
            domain: "https://example.com".parse().unwrap(),
            result: ValidationResult {
                valid: true,
                error: None,
                last_validated_at: fixed_time(),
            },
        })]
    }

    fn pending_changes() -> PendingChanges {
        PendingChanges {
            dids: Some(vec![TEST_DID.parse().unwrap()]),
            display: Some(display()),
        }
    }

    fn all_variants() -> Vec<ConnectionEvent> {
        vec![
            ConnectionEvent::ConnectionAdded {
                connection_id: "connection-1".to_string(),
                display: Some(display()),
                url: "https://example.com".parse().unwrap(),
                dids: vec![TEST_DID.parse().unwrap()],
                first_interacted_at: Some(fixed_time()),
                last_interacted_at: Some(fixed_time()),
                validations: validations(),
            },
            ConnectionEvent::ConnectionRemoved {
                connection_id: "connection-1".to_string(),
            },
            ConnectionEvent::ConnectionSynced {
                connection_id: "connection-1".to_string(),
                validations: validations(),
                pending_changes: Some(pending_changes()),
                last_interacted_at: Some(fixed_time()),
            },
            ConnectionEvent::ConnectionChangesAccepted {
                connection_id: "connection-1".to_string(),
                display: Some(display()),
                dids: vec![TEST_DID.parse().unwrap()],
                last_interacted_at: Some(fixed_time()),
                pending_changes: Some(pending_changes()),
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: ConnectionEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_connection_added() {
        let golden = serde_json::json!({
            "ConnectionAdded": {
                "connection_id": "connection-1",
                "display": {
                    "name": "Time Regulation Institute",
                    "locale": "en",
                    "logo": {
                        "uri": "https://example.com/logo.png",
                        "alt_text": "Organisational Logo"
                    }
                },
                "url": "https://example.com/",
                "dids": ["did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"],
                "first_interacted_at": "2023-11-14T22:13:20Z",
                "last_interacted_at": "2023-11-14T22:13:20Z",
                "validations": [
                    {
                        "DomainLinkage": {
                            "domain": "https://example.com/",
                            "result": {
                                "valid": true,
                                "error": null,
                                "last_validated_at": "2023-11-14T22:13:20Z"
                            }
                        }
                    }
                ]
            }
        });

        let event: ConnectionEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_connection_removed() {
        let golden = serde_json::json!({
            "ConnectionRemoved": {
                "connection_id": "connection-1"
            }
        });

        let event: ConnectionEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_connection_synced() {
        let golden = serde_json::json!({
            "ConnectionSynced": {
                "connection_id": "connection-1",
                "validations": [
                    {
                        "DomainLinkage": {
                            "domain": "https://example.com/",
                            "result": {
                                "valid": true,
                                "error": null,
                                "last_validated_at": "2023-11-14T22:13:20Z"
                            }
                        }
                    }
                ],
                "pending_changes": {
                    "dids": ["did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"],
                    "display": {
                        "name": "Time Regulation Institute",
                        "locale": "en",
                        "logo": {
                            "uri": "https://example.com/logo.png",
                            "alt_text": "Organisational Logo"
                        }
                    }
                },
                "last_interacted_at": "2023-11-14T22:13:20Z"
            }
        });

        let event: ConnectionEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_connection_changes_accepted() {
        let golden = serde_json::json!({
            "ConnectionChangesAccepted": {
                "connection_id": "connection-1",
                "display": {
                    "name": "Time Regulation Institute",
                    "locale": "en",
                    "logo": {
                        "uri": "https://example.com/logo.png",
                        "alt_text": "Organisational Logo"
                    }
                },
                "dids": ["did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"],
                "last_interacted_at": "2023-11-14T22:13:20Z",
                "pending_changes": {
                    "dids": ["did:key:z6MkoTHsgNNrby8JzCNQ1iRLyW5QQ6R8Xuu6AA8igGrMVPUM"],
                    "display": {
                        "name": "Time Regulation Institute",
                        "locale": "en",
                        "logo": {
                            "uri": "https://example.com/logo.png",
                            "alt_text": "Organisational Logo"
                        }
                    }
                }
            }
        });

        let event: ConnectionEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
