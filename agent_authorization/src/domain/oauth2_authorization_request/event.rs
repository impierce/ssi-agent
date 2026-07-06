use crate::domain::oauth2_authorization_request::aggregate::ConsentStatus;
use cqrs_es::DomainEvent;
use oid4vci::{authorization_details::AuthorizationDetailsObject, authorization_request::CodeChallengeMethod};
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

// TODO: remove this clippy allow
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OAuth2AuthorizationRequestEvent {
    OAuth2AuthorizationRequestCreated {
        oauth2_authorization_request_id: String,
        response_type: String,
        state: String,
        client_id: String,
        redirect_uri: Option<Url>,
        scope: Option<String>,
        issuer_state: Option<String>,

        // OID4VCI
        authorization_details: Option<Vec<AuthorizationDetailsObject>>,

        // PKCE
        #[serde(default)]
        code_challenge: Option<String>,
        #[serde(default)]
        code_challenge_method: Option<CodeChallengeMethod>,

        expires_at: i64,

        openid4vp_request: Option<serde_json::Value>,
    },
    OAuth2AuthorizationRequestExpired {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
    },
    ConsentGranted {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
    },
    ConsentRejected {
        oauth2_authorization_request_id: String,
        consent_status: ConsentStatus,
    },
}

impl DomainEvent for OAuth2AuthorizationRequestEvent {
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
    use oid4vci::authorization_details::OpenidCredential;

    fn authorization_details() -> Vec<AuthorizationDetailsObject> {
        vec![AuthorizationDetailsObject {
            r#type: OpenidCredential::Type,
            locations: None,
            credential_configuration_id: "001".to_string(),
            credential_identifiers: None,
            claims: None,
        }]
    }

    fn all_variants() -> Vec<OAuth2AuthorizationRequestEvent> {
        vec![
            OAuth2AuthorizationRequestEvent::OAuth2AuthorizationRequestCreated {
                oauth2_authorization_request_id: "oauth2_authorization_request_id".to_string(),
                response_type: "code".to_string(),
                state: "state".to_string(),
                client_id: "client_id".to_string(),
                redirect_uri: Some("https://client.example.test/cb".parse().unwrap()),
                scope: Some("openid profile".to_string()),
                issuer_state: Some("issuer_state_def".to_string()),
                authorization_details: Some(authorization_details()),
                code_challenge: Some("code_challenge".to_string()),
                code_challenge_method: Some(CodeChallengeMethod::S256),
                expires_at: 1700000000,
                openid4vp_request: Some(serde_json::json!({"client_id": "verifier"})),
            },
            OAuth2AuthorizationRequestEvent::OAuth2AuthorizationRequestExpired {
                oauth2_authorization_request_id: "oauth2_authorization_request_id".to_string(),
                consent_status: ConsentStatus::Expired,
            },
            OAuth2AuthorizationRequestEvent::ConsentGranted {
                oauth2_authorization_request_id: "oauth2_authorization_request_id".to_string(),
                consent_status: ConsentStatus::Granted,
            },
            OAuth2AuthorizationRequestEvent::ConsentRejected {
                oauth2_authorization_request_id: "oauth2_authorization_request_id".to_string(),
                consent_status: ConsentStatus::Rejected,
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: OAuth2AuthorizationRequestEvent = serde_json::from_value(value).unwrap();
            assert_eq!(event, deserialized);
        }
    }

    #[test]
    fn golden_oauth2_authorization_request_created() {
        let golden = serde_json::json!({
            "OAuth2AuthorizationRequestCreated": {
                "oauth2_authorization_request_id": "oauth2_authorization_request_id",
                "response_type": "code",
                "state": "state",
                "client_id": "client_id",
                "redirect_uri": "https://client.example.test/cb",
                "scope": "openid profile",
                "issuer_state": "issuer_state_def",
                "authorization_details": [
                    {
                        "type": "openid_credential",
                        "credential_configuration_id": "001"
                    }
                ],
                "code_challenge": "code_challenge",
                "code_challenge_method": "S256",
                "expires_at": 1700000000,
                "openid4vp_request": { "client_id": "verifier" }
            }
        });

        let event: OAuth2AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_oauth2_authorization_request_expired() {
        let golden = serde_json::json!({
            "OAuth2AuthorizationRequestExpired": {
                "oauth2_authorization_request_id": "oauth2_authorization_request_id",
                "consent_status": "Expired"
            }
        });

        let event: OAuth2AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_consent_granted() {
        let golden = serde_json::json!({
            "ConsentGranted": {
                "oauth2_authorization_request_id": "oauth2_authorization_request_id",
                "consent_status": "Granted"
            }
        });

        let event: OAuth2AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_consent_rejected() {
        let golden = serde_json::json!({
            "ConsentRejected": {
                "oauth2_authorization_request_id": "oauth2_authorization_request_id",
                "consent_status": "Rejected"
            }
        });

        let event: OAuth2AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
