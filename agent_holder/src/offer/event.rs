use super::aggregate::{OfferCredential, Status};
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_offer::CredentialOfferParameters, token_response::TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OfferEvent {
    CredentialOfferReceived {
        received_offer_id: String,
        credential_offer: Box<CredentialOfferParameters>,
        credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject>,
    },
    CredentialOfferAccepted {
        received_offer_id: String,
        status: Status,
    },
    TokenResponseReceived {
        received_offer_id: String,
        token_response: TokenResponse,
    },
    CredentialResponseReceived {
        received_offer_id: String,
        status: Status,
        credentials: Vec<OfferCredential>,
    },
    CredentialOfferRejected {
        received_offer_id: String,
        status: Status,
    },
}

impl DomainEvent for OfferEvent {
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
    use identity_credential::credential::Jwt;
    use oid4vci::credential_offer::{CredentialConfigurationIds, Grants, PreAuthorizedCode};
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: OfferEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: OfferEvent = serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: OfferEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(from_golden, event, "golden fixture no longer deserializes into the expected event");
    }

    fn fixed_url() -> reqwest::Url {
        "https://my-domain.example.org/".parse().unwrap()
    }

    fn fixed_credential_offer_parameters() -> Box<CredentialOfferParameters> {
        Box::new(CredentialOfferParameters {
            credential_issuer: fixed_url(),
            credential_configuration_ids: CredentialConfigurationIds::try_new(vec!["UniversityDegree".to_string()])
                .unwrap(),
            grants: Some(Grants {
                authorization_code: None,
                pre_authorized_code: Some(PreAuthorizedCode {
                    pre_authorized_code: "test-pre-authorized-code".to_string(),
                    ..Default::default()
                }),
            }),
        })
    }

    fn fixed_credential_offer_parameters_json() -> serde_json::Value {
        json!({
            "credential_issuer": "https://my-domain.example.org/",
            "credential_configuration_ids": ["UniversityDegree"],
            "grants": {
                "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                    "pre-authorized_code": "test-pre-authorized-code"
                }
            }
        })
    }

    #[test]
    fn credential_offer_received() {
        let event = OfferEvent::CredentialOfferReceived {
            received_offer_id: "received-offer-id".to_string(),
            credential_offer: fixed_credential_offer_parameters(),
            credential_configurations: HashMap::new(),
        };
        let golden = json!({
            "CredentialOfferReceived": {
                "received_offer_id": "received-offer-id",
                "credential_offer": fixed_credential_offer_parameters_json(),
                "credential_configurations": {}
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_offer_accepted() {
        let event = OfferEvent::CredentialOfferAccepted {
            received_offer_id: "received-offer-id".to_string(),
            status: Status::Accepted,
        };
        let golden = json!({
            "CredentialOfferAccepted": {
                "received_offer_id": "received-offer-id",
                "status": "Accepted"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn token_response_received() {
        let event = OfferEvent::TokenResponseReceived {
            received_offer_id: "received-offer-id".to_string(),
            token_response: TokenResponse {
                access_token: "access-token".to_string(),
                token_type: "Bearer".to_string(),
                expires_in: None,
                refresh_token: None,
                scope: None,
                authorization_details: None,
            },
        };
        let golden = json!({
            "TokenResponseReceived": {
                "received_offer_id": "received-offer-id",
                "token_response": {
                    "access_token": "access-token",
                    "token_type": "Bearer"
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_response_received() {
        let event = OfferEvent::CredentialResponseReceived {
            received_offer_id: "received-offer-id".to_string(),
            status: Status::CredentialsReceived,
            credentials: vec![OfferCredential {
                holder_credential_id: "holder-credential-id".to_string(),
                credential: Jwt::from("eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ0ZXN0In0.sig".to_string()),
            }],
        };
        let golden = json!({
            "CredentialResponseReceived": {
                "received_offer_id": "received-offer-id",
                "status": "CredentialsReceived",
                "credentials": [
                    {
                        "holder_credential_id": "holder-credential-id",
                        "credential": "eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ0ZXN0In0.sig"
                    }
                ]
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_offer_rejected() {
        let event = OfferEvent::CredentialOfferRejected {
            received_offer_id: "received-offer-id".to_string(),
            status: Status::Rejected,
        };
        let golden = json!({
            "CredentialOfferRejected": {
                "received_offer_id": "received-offer-id",
                "status": "Rejected"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
