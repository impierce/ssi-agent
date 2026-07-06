use crate::credential::aggregate::CredentialStatus;

use super::{aggregate::Status, entity::Data};
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    notification_request::NotificationRequest,
};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum CredentialEvent {
    // TODO: rename to `DataCredentialCreated`?
    UnsignedCredentialCreated {
        credential_id: String,
        data: Data,
        notification_id: Option<String>,
        credential_configuration: Box<CredentialConfigurationsSupportedObject>,
        created_at: Option<DateTime<Utc>>,
        expires_at: Option<DateTime<Utc>>,
    },
    SignedCredentialCreated {
        credential_id: String,
        signed_credential: serde_json::Value,
        notification_id: Option<String>,
    },
    CredentialSigned {
        credential_id: String,
        signed_credential: serde_json::Value,
        credential_status: CredentialStatus,
        status: Status,
    },
    NotificationReceived {
        credential_id: String,
        notification: NotificationRequest,
    },
    CredentialStatusUpdated {
        credential_id: String,
        credential_status: CredentialStatus,
    },
}

impl DomainEvent for CredentialEvent {
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

/// Wire-format regression tests: every variant is round-tripped through JSON and checked
/// against a checked-in "golden" JSON literal. If a golden fixture stops matching, either the
/// change is breaking (bump `event_version` and add an upcaster, see `docs/event-versioning.md`)
/// or the fixture needs deliberate updating.
#[cfg(test)]
mod wire_format_tests {
    use super::*;
    use oauth_tsl::status_list::StatusType;
    use oid4vci::notification_request::NotificationEvent;
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: CredentialEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: CredentialEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: CredentialEvent =
            serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    fn fixed_credential_configuration_json() -> serde_json::Value {
        json!({
            "format": "jwt_vc_json",
            "credential_definition": { "type": ["VerifiableCredential"] },
            "cryptographic_binding_methods_supported": ["did:jwk", "did:key"],
            "credential_signing_alg_values_supported": ["ES256", "EdDSA"],
            "proof_types_supported": { "jwt": { "proof_signing_alg_values_supported": ["ES256", "EdDSA"] } },
            "credential_metadata": {
                "display": [
                    {
                        "name": "Verifiable Credential",
                        "locale": "en",
                        "logo": {
                            "uri": "https://www.impierce.com/external/impierce-logo.png",
                            "alt_text": "Impierce Logo"
                        }
                    }
                ]
            }
        })
    }

    #[test]
    fn unsigned_credential_created() {
        use crate::credential::aggregate::test_utils::JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION;

        let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z".parse().unwrap();
        let event = CredentialEvent::UnsignedCredentialCreated {
            credential_id: "credential-id".to_string(),
            data: Data {
                raw: json!({"first_name": "Ferris"}),
            },
            notification_id: Some("notification-id".to_string()),
            credential_configuration: Box::new(JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION.clone()),
            created_at: Some(created_at),
            expires_at: None,
        };
        let golden = json!({
            "UnsignedCredentialCreated": {
                "credential_id": "credential-id",
                "data": { "raw": {"first_name": "Ferris"} },
                "notification_id": "notification-id",
                "credential_configuration": fixed_credential_configuration_json(),
                "created_at": "2010-01-01T00:00:00Z",
                "expires_at": null
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn signed_credential_created() {
        let event = CredentialEvent::SignedCredentialCreated {
            credential_id: "credential-id".to_string(),
            signed_credential: json!("signed-jwt"),
            notification_id: Some("notification-id".to_string()),
        };
        let golden = json!({
            "SignedCredentialCreated": {
                "credential_id": "credential-id",
                "signed_credential": "signed-jwt",
                "notification_id": "notification-id"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_signed() {
        let event = CredentialEvent::CredentialSigned {
            credential_id: "credential-id".to_string(),
            signed_credential: json!("signed-jwt"),
            credential_status: CredentialStatus {
                index: 5,
                status: StatusType::VALID,
                status_list_url: "https://my-domain.example.org/ietf-oauth-token-status-list/0".to_string(),
            },
            status: Status::Issued,
        };
        let golden = json!({
            "CredentialSigned": {
                "credential_id": "credential-id",
                "signed_credential": "signed-jwt",
                "credential_status": {
                    "index": 5,
                    "status": "VALID",
                    "status_list_url": "https://my-domain.example.org/ietf-oauth-token-status-list/0"
                },
                "status": "Issued"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn notification_received() {
        let event = CredentialEvent::NotificationReceived {
            credential_id: "credential-id".to_string(),
            notification: NotificationRequest {
                notification_id: "notification-id".to_string(),
                event: NotificationEvent::CredentialAccepted,
                event_description: None,
            },
        };
        let golden = json!({
            "NotificationReceived": {
                "credential_id": "credential-id",
                "notification": {
                    "notification_id": "notification-id",
                    "event": "credential_accepted"
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_status_updated() {
        let event = CredentialEvent::CredentialStatusUpdated {
            credential_id: "credential-id".to_string(),
            credential_status: CredentialStatus {
                index: 7,
                status: StatusType::SUSPENDED,
                status_list_url: "https://my-domain.example.org/ietf-oauth-token-status-list/0".to_string(),
            },
        };
        let golden = json!({
            "CredentialStatusUpdated": {
                "credential_id": "credential-id",
                "credential_status": {
                    "index": 7,
                    "status": "SUSPENDED",
                    "status_list_url": "https://my-domain.example.org/ietf-oauth-token-status-list/0"
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
