use super::aggregate::Status;
use crate::offer::aggregate::DeliveryOptions;
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_offer::{CredentialOffer, GrantType},
    credential_response::CredentialResponse,
};
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OfferEvent {
    CredentialOfferCreated {
        offer_id: String,
        grant_types: Vec<GrantType>,
        credential_offer: CredentialOffer,
        credential_offer_uri: CredentialOffer,
        pre_authorized_code: String,
        status: Status,
        tx_code: Option<String>,
        delivery_options: Option<DeliveryOptions>,
    },
    CredentialsAdded {
        offer_id: String,
        credential_ids: Vec<String>,
        credential_offer: CredentialOffer,
    },
    FormUrlEncodedCredentialOfferCreated {
        offer_id: String,
        form_url_encoded_credential_offer: String,
        status: Status,
    },
    CredentialOfferSent {
        offer_id: String,
        target_url: Url,
        status: Status,
    },
    CredentialOfferEmailSent {
        offer_id: String,
        recipient_email: String,
        form_url_encoded_credential_offer: String,
        offer_link: Url,
        status: Status,
    },
    CredentialRequestVerified {
        offer_id: String,
        subject_id: Option<String>,
    },
    CredentialResponseCreated {
        offer_id: String,
        credential_response: CredentialResponse,
        status: Status,
    },
    TxCodeGenerated {
        offer_id: String,
        tx_code: String,
        delivery_options: Option<DeliveryOptions>,
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
    use oid4vci::credential_offer::{CredentialConfigurationIds, CredentialOfferParameters, Grants, PreAuthorizedCode};
    use oid4vci::credential_response::{CredentialResponseObject, CredentialResponseType};
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: OfferEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: OfferEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: OfferEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(
            from_golden, event,
            "golden fixture no longer deserializes into the expected event"
        );
    }

    fn fixed_url() -> url::Url {
        "https://my-domain.example.org/".parse().unwrap()
    }

    fn fixed_credential_offer() -> CredentialOffer {
        CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
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
        }))
    }

    fn fixed_credential_offer_json() -> serde_json::Value {
        json!({
            "credential_offer": {
                "credential_issuer": "https://my-domain.example.org/",
                "credential_configuration_ids": ["UniversityDegree"],
                "grants": {
                    "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                        "pre-authorized_code": "test-pre-authorized-code"
                    }
                }
            }
        })
    }

    #[test]
    fn credential_offer_created() {
        let event = OfferEvent::CredentialOfferCreated {
            offer_id: "offer-id".to_string(),
            grant_types: vec![GrantType::PreAuthorizedCode],
            credential_offer: fixed_credential_offer(),
            credential_offer_uri: CredentialOffer::CredentialOfferUri(
                fixed_url().join("credential-offer/offer-id").unwrap(),
            ),
            pre_authorized_code: "test-pre-authorized-code".to_string(),
            status: Status::Created,
            tx_code: None,
            delivery_options: None,
        };
        let golden = json!({
            "CredentialOfferCreated": {
                "offer_id": "offer-id",
                "grant_types": ["urn:ietf:params:oauth:grant-type:pre-authorized_code"],
                "credential_offer": fixed_credential_offer_json(),
                "credential_offer_uri": { "credential_offer_uri": "https://my-domain.example.org/credential-offer/offer-id" },
                "pre_authorized_code": "test-pre-authorized-code",
                "status": "Created",
                "tx_code": null,
                "delivery_options": null
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credentials_added() {
        let event = OfferEvent::CredentialsAdded {
            offer_id: "offer-id".to_string(),
            credential_ids: vec!["credential-id".to_string()],
            credential_offer: fixed_credential_offer(),
        };
        let golden = json!({
            "CredentialsAdded": {
                "offer_id": "offer-id",
                "credential_ids": ["credential-id"],
                "credential_offer": fixed_credential_offer_json()
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn form_url_encoded_credential_offer_created() {
        let event = OfferEvent::FormUrlEncodedCredentialOfferCreated {
            offer_id: "offer-id".to_string(),
            form_url_encoded_credential_offer: "openid-credential-offer://?credential_offer=...".to_string(),
            status: Status::Pending,
        };
        let golden = json!({
            "FormUrlEncodedCredentialOfferCreated": {
                "offer_id": "offer-id",
                "form_url_encoded_credential_offer": "openid-credential-offer://?credential_offer=...",
                "status": "Pending"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_offer_sent() {
        let event = OfferEvent::CredentialOfferSent {
            offer_id: "offer-id".to_string(),
            target_url: fixed_url(),
            status: Status::Pending,
        };
        let golden = json!({
            "CredentialOfferSent": {
                "offer_id": "offer-id",
                "target_url": "https://my-domain.example.org/",
                "status": "Pending"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_offer_email_sent() {
        let event = OfferEvent::CredentialOfferEmailSent {
            offer_id: "offer-id".to_string(),
            recipient_email: "test@example.com".to_string(),
            form_url_encoded_credential_offer: "openid-credential-offer://?credential_offer=...".to_string(),
            offer_link: fixed_url().join("offer/offer-id").unwrap(),
            status: Status::Pending,
        };
        let golden = json!({
            "CredentialOfferEmailSent": {
                "offer_id": "offer-id",
                "recipient_email": "test@example.com",
                "form_url_encoded_credential_offer": "openid-credential-offer://?credential_offer=...",
                "offer_link": "https://my-domain.example.org/offer/offer-id",
                "status": "Pending"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_request_verified() {
        let event = OfferEvent::CredentialRequestVerified {
            offer_id: "offer-id".to_string(),
            subject_id: Some("did:key:test".to_string()),
        };
        let golden = json!({
            "CredentialRequestVerified": {
                "offer_id": "offer-id",
                "subject_id": "did:key:test"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_response_created() {
        let event = OfferEvent::CredentialResponseCreated {
            offer_id: "offer-id".to_string(),
            credential_response: CredentialResponse {
                credential: CredentialResponseType::Immediate {
                    credentials: vec![CredentialResponseObject {
                        credential: "jwt-credential".to_string(),
                    }],
                    notification_id: Some("notification-id".to_string()),
                },
            },
            status: Status::Issued,
        };
        let golden = json!({
            "CredentialResponseCreated": {
                "offer_id": "offer-id",
                "credential_response": {
                    "credentials": [ { "credential": "jwt-credential" } ],
                    "notification_id": "notification-id"
                },
                "status": "Issued"
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn tx_code_generated() {
        let event = OfferEvent::TxCodeGenerated {
            offer_id: "offer-id".to_string(),
            tx_code: "123456".to_string(),
            delivery_options: Some(DeliveryOptions {
                recipient_email: Some("test@example.com".to_string()),
            }),
        };
        let golden = json!({
            "TxCodeGenerated": {
                "offer_id": "offer-id",
                "tx_code": "123456",
                "delivery_options": { "recipient_email": "test@example.com" }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
