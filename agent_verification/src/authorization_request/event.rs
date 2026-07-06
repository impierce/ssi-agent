use crate::generic_oid4vc::GenericAuthorizationRequest;
use cqrs_es::DomainEvent;
use oid4vp::token::vp_token_validator::DecodedVpToken;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, strum::Display)]
pub enum AuthorizationRequestEvent {
    AuthorizationRequestCreated {
        authorization_request: Box<GenericAuthorizationRequest>,
    },
    FormUrlEncodedAuthorizationRequestCreated {
        form_url_encoded_authorization_request: String,
    },
    AuthorizationRequestObjectSigned {
        signed_authorization_request_object: String,
    },
    SIOPv2AuthorizationResponseVerified {
        id_token: String,
        state: Option<String>,
        validated: bool,
    },
    OID4VPAuthorizationResponseVerified {
        vp_token: DecodedVpToken,
        state: Option<String>,
        validated: bool,
    },
}

impl DomainEvent for AuthorizationRequestEvent {
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
    use crate::generic_oid4vc::{OID4VPAuthorizationRequest, SIOPv2AuthorizationRequest};
    use oid4vc_core::client_metadata::ClientMetadataResource;
    use oid4vc_core::scope::Scope;
    use oid4vp::authorization_request::ClientId;
    use oid4vp::dcql::dcql_query::{CredentialQuery, CredentialQueryId, DcqlQuery, Format, MetaTypes};
    use oid4vp::token::vp_token_validator::DecodedVpTokenBuilder;
    use std::str::FromStr;

    const VERIFIER_DID: &str = "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt";

    fn siopv2_request() -> SIOPv2AuthorizationRequest {
        SIOPv2AuthorizationRequest::builder()
            .client_id(VERIFIER_DID.to_string())
            .scope(Scope::openid())
            .redirect_uri("https://my-domain.example.org/redirect".parse::<url::Url>().unwrap())
            .response_mode("direct_post".to_string())
            .client_metadata(ClientMetadataResource::ClientMetadataUri(
                "https://my-domain.example.org/client-metadata".to_string(),
            ))
            .nonce("nonce".to_string())
            .state("state".to_string())
            .build()
            .unwrap()
    }

    fn dcql_query() -> DcqlQuery {
        DcqlQuery {
            credentials: vec![CredentialQuery {
                id: CredentialQueryId::try_new("CredentialQuery".to_string()).unwrap(),
                format: Format::JwtVcJson,
                multiple: None,
                meta: MetaTypes::W3CFormatMeta {
                    type_values: vec![vec!["VerifiableCredential".to_string()]],
                },
                trusted_authorities: None,
                require_cryptographic_holder_binding: Some(true),
                claims: None,
                claim_sets: None,
            }],
            credential_sets: None,
        }
    }

    fn oid4vp_request() -> OID4VPAuthorizationRequest {
        OID4VPAuthorizationRequest::builder()
            .dcql_query(dcql_query())
            .client_id(ClientId::from_str(&format!("decentralized_identifier:{VERIFIER_DID}")).unwrap())
            .scope(Scope::openid())
            .response_uri("https://my-domain.example.org/redirect".parse::<url::Url>().unwrap())
            .response_mode("direct_post".to_string())
            .client_metadata(ClientMetadataResource::ClientMetadataUri(
                "https://my-domain.example.org/client-metadata".to_string(),
            ))
            .nonce("nonce".to_string())
            .state("state".to_string())
            .build()
            .unwrap()
    }

    fn decoded_vp_token() -> DecodedVpToken {
        DecodedVpTokenBuilder::new()
            .insert(
                CredentialQueryId::try_new("CredentialQuery").unwrap(),
                vec![serde_json::json!({
                    "@context": ["https://www.w3.org/2018/credentials/v1"],
                    "type": ["VerifiableCredential", "PersonalInformation"],
                    "credentialSubject": {
                        "id": "did:key:z6MkmDhE9TjaaME2ApzpWx7g2xZE3zivnEJdZT8avPUBZjuE",
                        "givenName": "Ferris"
                    }
                })
                .as_object()
                .unwrap()
                .clone()],
            )
            .unwrap()
            .build()
    }

    fn all_variants() -> Vec<AuthorizationRequestEvent> {
        vec![
            AuthorizationRequestEvent::AuthorizationRequestCreated {
                authorization_request: Box::new(GenericAuthorizationRequest::SIOPv2(Box::new(siopv2_request()))),
            },
            AuthorizationRequestEvent::AuthorizationRequestCreated {
                authorization_request: Box::new(GenericAuthorizationRequest::OID4VP(Box::new(oid4vp_request()))),
            },
            AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request: "openid://?client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate".to_string(),
            },
            AuthorizationRequestEvent::AuthorizationRequestObjectSigned {
                signed_authorization_request_object: "dummy.signed.jwt".to_string(),
            },
            AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
                id_token: "dummy.id.jwt".to_string(),
                state: Some("state".to_string()),
                validated: true,
            },
            AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified {
                vp_token: decoded_vp_token(),
                state: Some("state".to_string()),
                validated: true,
            },
        ]
    }

    #[test]
    fn round_trips_every_variant() {
        // `GenericAuthorizationRequest`'s inner `custom_url_scheme` field is `#[serde(skip)]`,
        // so it never round-trips through JSON (it is reset to its `Default` on deserialize).
        // We therefore compare the re-serialized JSON values, which is also the more faithful
        // check of the persisted wire format.
        for event in all_variants() {
            let value = serde_json::to_value(&event).unwrap();
            let deserialized: AuthorizationRequestEvent = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&deserialized).unwrap(), value);
        }
    }

    #[test]
    fn golden_authorization_request_created_siopv2() {
        let golden = serde_json::json!({
            "AuthorizationRequestCreated": {
                "authorization_request": {
                    "client_id": "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt",
                    "redirect_uri": "https://my-domain.example.org/redirect",
                    "state": "state",
                    "response_type": "id_token",
                    "scope": "openid",
                    "response_mode": "direct_post",
                    "nonce": "nonce",
                    "client_metadata_uri": "https://my-domain.example.org/client-metadata"
                }
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_authorization_request_created_oid4vp() {
        let golden = serde_json::json!({
            "AuthorizationRequestCreated": {
                "authorization_request": {
                    "client_id": "decentralized_identifier:did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt",
                    "response_uri": "https://my-domain.example.org/redirect",
                    "state": "state",
                    "response_type": "vp_token",
                    "dcql_query": {
                        "credentials": [
                            {
                                "id": "CredentialQuery",
                                "format": "jwt_vc_json",
                                "meta": {
                                    "type_values": [["VerifiableCredential"]]
                                },
                                "require_cryptographic_holder_binding": true
                            }
                        ]
                    },
                    "response_mode": "direct_post",
                    "scope": "openid",
                    "nonce": "nonce",
                    "client_metadata_uri": "https://my-domain.example.org/client-metadata"
                }
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_form_url_encoded_authorization_request_created() {
        let golden = serde_json::json!({
            "FormUrlEncodedAuthorizationRequestCreated": {
                "form_url_encoded_authorization_request": "openid://?client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_authorization_request_object_signed() {
        let golden = serde_json::json!({
            "AuthorizationRequestObjectSigned": {
                "signed_authorization_request_object": "dummy.signed.jwt"
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_siopv2_authorization_response_verified() {
        let golden = serde_json::json!({
            "SIOPv2AuthorizationResponseVerified": {
                "id_token": "dummy.id.jwt",
                "state": "state",
                "validated": true
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }

    #[test]
    fn golden_oid4vp_authorization_response_verified() {
        let golden = serde_json::json!({
            "OID4VPAuthorizationResponseVerified": {
                "vp_token": {
                    "CredentialQuery": [
                        {
                            "@context": ["https://www.w3.org/2018/credentials/v1"],
                            "type": ["VerifiableCredential", "PersonalInformation"],
                            "credentialSubject": {
                                "id": "did:key:z6MkmDhE9TjaaME2ApzpWx7g2xZE3zivnEJdZT8avPUBZjuE",
                                "givenName": "Ferris"
                            }
                        }
                    ]
                },
                "state": "state",
                "validated": true
            }
        });

        let event: AuthorizationRequestEvent = serde_json::from_value(golden.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), golden);
    }
}
