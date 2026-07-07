use agent_shared::config::Authorization;
use cqrs_es::DomainEvent;
use jsonwebtoken::Algorithm;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata,
    credential_configurations_supported::CredentialConfigurationsSupportedObject,
    credential_issuer_metadata::CredentialIssuerMetadata,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ServerConfigEvent {
    ServerMetadataInitialized {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        cryptographic_binding_methods_supported: Vec<String>,
        signing_algorithms_supported: Vec<Algorithm>,
    },
    IssuerUrlUpdated {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    },
    IssuerDisplayUpdated {
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    },
    CryptographicBindingMethodsUpdated {
        cryptographic_binding_methods_supported: Vec<String>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    SigningAlgorithmsUpdated {
        signing_algorithms_supported: Vec<Algorithm>,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationUpdated {
        credential_configuration_id: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
    },
    CredentialConfigurationRemoved {
        credential_configuration_id: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        credential_configurations: HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)>,
    },
}

impl DomainEvent for ServerConfigEvent {
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
    use serde_json::json;

    /// Asserts that `event` serializes to exactly `golden`, that it round-trips losslessly
    /// through JSON, and that the golden fixture itself still deserializes into `event`.
    fn assert_round_trip_and_golden(event: ServerConfigEvent, golden: serde_json::Value) {
        let serialized = serde_json::to_value(&event).expect("event should serialize");
        assert_eq!(serialized, golden, "serialized event drifted from the golden fixture");

        let round_tripped: ServerConfigEvent =
            serde_json::from_value(serialized).expect("serialized event should deserialize");
        assert_eq!(round_tripped, event, "round-trip through JSON changed the event");

        let from_golden: ServerConfigEvent = serde_json::from_value(golden).expect("golden fixture should deserialize");
        assert_eq!(
            from_golden, event,
            "golden fixture no longer deserializes into the expected event"
        );
    }

    fn fixed_url() -> url::Url {
        "https://my-domain.example.org/".parse().unwrap()
    }

    fn fixed_authorization_server_metadata() -> Box<AuthorizationServerMetadata> {
        Box::new(AuthorizationServerMetadata {
            issuer: fixed_url(),
            token_endpoint: Some(fixed_url().join("token").unwrap()),
            ..Default::default()
        })
    }

    fn fixed_credential_issuer_metadata() -> Box<CredentialIssuerMetadata> {
        Box::new(CredentialIssuerMetadata {
            credential_issuer: fixed_url(),
            credential_endpoint: fixed_url().join("credential").unwrap(),
            ..Default::default()
        })
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

    fn fixed_credential_configurations(
    ) -> HashMap<String, (bool, CredentialConfigurationsSupportedObject, Authorization)> {
        use crate::credential::aggregate::test_utils::JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION;
        HashMap::from_iter(vec![(
            "001".to_string(),
            (
                false,
                JWT_VC_JSON_VC1_1_CREDENTIAL_CONFIGURATION.clone(),
                Authorization {
                    pre_authorized: true,
                    tx_code_constraints: None,
                },
            ),
        )])
    }

    #[test]
    fn server_metadata_initialized() {
        let event = ServerConfigEvent::ServerMetadataInitialized {
            authorization_server_metadata: fixed_authorization_server_metadata(),
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
            cryptographic_binding_methods_supported: vec!["did:jwk".to_string()],
            signing_algorithms_supported: vec![Algorithm::EdDSA],
        };
        let golden = json!({
            "ServerMetadataInitialized": {
                "authorization_server_metadata": {
                    "issuer": "https://my-domain.example.org/",
                    "token_endpoint": "https://my-domain.example.org/token"
                },
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                },
                "cryptographic_binding_methods_supported": ["did:jwk"],
                "signing_algorithms_supported": ["EdDSA"]
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn issuer_url_updated() {
        let event = ServerConfigEvent::IssuerUrlUpdated {
            authorization_server_metadata: fixed_authorization_server_metadata(),
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
        };
        let golden = json!({
            "IssuerUrlUpdated": {
                "authorization_server_metadata": {
                    "issuer": "https://my-domain.example.org/",
                    "token_endpoint": "https://my-domain.example.org/token"
                },
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn issuer_display_updated() {
        let event = ServerConfigEvent::IssuerDisplayUpdated {
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
        };
        let golden = json!({
            "IssuerDisplayUpdated": {
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn cryptographic_binding_methods_updated() {
        let event = ServerConfigEvent::CryptographicBindingMethodsUpdated {
            cryptographic_binding_methods_supported: vec!["did:jwk".to_string()],
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
            credential_configurations: fixed_credential_configurations(),
        };
        let golden = json!({
            "CryptographicBindingMethodsUpdated": {
                "cryptographic_binding_methods_supported": ["did:jwk"],
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                },
                "credential_configurations": {
                    "001": [false, fixed_credential_configuration_json(), { "pre_authorized": true }]
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn signing_algorithms_updated() {
        let event = ServerConfigEvent::SigningAlgorithmsUpdated {
            signing_algorithms_supported: vec![Algorithm::EdDSA],
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
            credential_configurations: fixed_credential_configurations(),
        };
        let golden = json!({
            "SigningAlgorithmsUpdated": {
                "signing_algorithms_supported": ["EdDSA"],
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                },
                "credential_configurations": {
                    "001": [false, fixed_credential_configuration_json(), { "pre_authorized": true }]
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_configuration_updated() {
        let event = ServerConfigEvent::CredentialConfigurationUpdated {
            credential_configuration_id: "001".to_string(),
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
            credential_configurations: fixed_credential_configurations(),
        };
        let golden = json!({
            "CredentialConfigurationUpdated": {
                "credential_configuration_id": "001",
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                },
                "credential_configurations": {
                    "001": [false, fixed_credential_configuration_json(), { "pre_authorized": true }]
                }
            }
        });
        assert_round_trip_and_golden(event, golden);
    }

    #[test]
    fn credential_configuration_removed() {
        let event = ServerConfigEvent::CredentialConfigurationRemoved {
            credential_configuration_id: "001".to_string(),
            credential_issuer_metadata: fixed_credential_issuer_metadata(),
            credential_configurations: HashMap::new(),
        };
        let golden = json!({
            "CredentialConfigurationRemoved": {
                "credential_configuration_id": "001",
                "credential_issuer_metadata": {
                    "credential_issuer": "https://my-domain.example.org/",
                    "credential_endpoint": "https://my-domain.example.org/credential",
                    "credential_configurations_supported": {}
                },
                "credential_configurations": {}
            }
        });
        assert_round_trip_and_golden(event, golden);
    }
}
