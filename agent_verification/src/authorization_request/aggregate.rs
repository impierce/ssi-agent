use super::{command::AuthorizationRequestCommand, error::AuthorizationRequestError, event::AuthorizationRequestEvent};
use crate::{
    generic_oid4vc::{GenericAuthorizationRequest, OID4VPAuthorizationRequest, SIOPv2AuthorizationRequest},
    services::VerificationServices,
};
use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::{authorization_request::ByReference, scope::Scope};
use oid4vp::authorization_request::ClientIdScheme;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AuthorizationRequest {
    authorization_request: Option<GenericAuthorizationRequest>,
    form_url_encoded_authorization_request: Option<String>,
    signed_authorization_request_object: Option<String>,
}

#[async_trait]
impl Aggregate for AuthorizationRequest {
    type Command = AuthorizationRequestCommand;
    type Event = AuthorizationRequestEvent;
    type Error = AuthorizationRequestError;
    type Services = Arc<VerificationServices>;

    fn aggregate_type() -> String {
        "authorization_request".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationRequestCommand::*;
        use AuthorizationRequestError::*;
        use AuthorizationRequestEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationRequest {
                state,
                nonce,
                presentation_definition,
            } => {
                let default_subject_syntax_type = services.relying_party.default_subject_syntax_type().to_string();
                let verifier = &services.verifier;
                let verifier_did = verifier.identifier(&default_subject_syntax_type).await.unwrap();

                let url = config!("url").unwrap();
                let request_uri = format!("{url}/request/{state}").parse().unwrap();
                let redirect_uri = format!("{url}/redirect").parse::<url::Url>().unwrap();

                let authorization_request = Box::new(if let Some(presentation_definition) = presentation_definition {
                    GenericAuthorizationRequest::OID4VP(Box::new(
                        OID4VPAuthorizationRequest::builder()
                            .client_id(verifier_did.clone())
                            .client_id_scheme(ClientIdScheme::Did)
                            .scope(Scope::openid())
                            .redirect_uri(redirect_uri)
                            .response_mode("direct_post".to_string())
                            .presentation_definition(presentation_definition)
                            .client_metadata(services.oid4vp_client_metadata.clone())
                            .state(state)
                            .nonce(nonce)
                            .build()
                            .map_err(AuthorizationRequestBuilderError)?,
                    ))
                } else {
                    GenericAuthorizationRequest::SIOPv2(Box::new(
                        SIOPv2AuthorizationRequest::builder()
                            .client_id(verifier_did.clone())
                            .scope(Scope::openid())
                            .redirect_uri(redirect_uri)
                            .response_mode("direct_post".to_string())
                            .client_metadata(services.siopv2_client_metadata.clone())
                            .state(state)
                            .nonce(nonce)
                            .build()
                            .map_err(AuthorizationRequestBuilderError)?,
                    ))
                });

                let form_url_encoded_authorization_request = oid4vc_core::authorization_request::AuthorizationRequest {
                    custom_url_scheme: "openid".to_string(),
                    body: ByReference {
                        client_id: verifier_did,
                        request_uri,
                    },
                }
                .to_string();

                Ok(vec![
                    AuthorizationRequestCreated { authorization_request },
                    FormUrlEncodedAuthorizationRequestCreated {
                        form_url_encoded_authorization_request,
                    },
                ])
            }
            SignAuthorizationRequestObject => {
                let relying_party = &services.relying_party;

                // TODO(oid4vc): This functionality should be moved to the `oid4vc-manager` crate.
                let authorization_request = self.authorization_request.as_ref().ok_or(MissingAuthorizationRequest)?;
                let signed_authorization_request_object = if let Some(siopv2_authorization_request) =
                    authorization_request.as_siopv2_authorization_request()
                {
                    relying_party
                        .encode(siopv2_authorization_request)
                        .await
                        .map_err(AuthorizationRequestSigningError)?
                } else if let Some(oid4vp_authorization_request) =
                    authorization_request.as_oid4vp_authorization_request()
                {
                    relying_party
                        .encode(oid4vp_authorization_request)
                        .await
                        .map_err(AuthorizationRequestSigningError)?
                } else {
                    unreachable!("`GenericAuthorizationRequest` cannot be `None`")
                };

                Ok(vec![AuthorizationRequestObjectSigned {
                    signed_authorization_request_object,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationRequestEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            AuthorizationRequestCreated { authorization_request } => {
                self.authorization_request.replace(*authorization_request);
            }
            FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request,
            } => {
                self.form_url_encoded_authorization_request
                    .replace(form_url_encoded_authorization_request);
            }
            AuthorizationRequestObjectSigned {
                signed_authorization_request_object,
            } => {
                self.signed_authorization_request_object
                    .replace(signed_authorization_request_object);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;
    use cqrs_es::test::TestFramework;
    use lazy_static::lazy_static;
    use oid4vc_core::Subject as _;
    use oid4vc_core::{client_metadata::ClientMetadataResource, SubjectSyntaxType};
    use oid4vp::PresentationDefinition;
    use rstest::rstest;
    use serde_json::json;

    use crate::services::test_utils::test_verification_services;

    use super::*;

    type AuthorizationRequestTestFramework = TestFramework<AuthorizationRequest>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_authorization_request(
        #[values("did:key", "did:jwk", "did:iota:rms")] verifier_did_method: &str,
    ) {
        let verification_services = test_verification_services(verifier_did_method);

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::CreateAuthorizationRequest {
                state: "state".to_string(),
                nonce: "nonce".to_string(),
                presentation_definition: None,
            })
            .then_expect_events(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(authorization_request("id_token", verifier_did_method).await),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(verifier_did_method),
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_sign_authorization_request_object(
        #[values("did:key", "did:jwk", "did:iota:rms")] verifier_did_method: &str,
    ) {
        let verification_services = test_verification_services(verifier_did_method);

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(authorization_request("id_token", verifier_did_method).await),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(verifier_did_method),
                },
            ])
            .when(AuthorizationRequestCommand::SignAuthorizationRequestObject)
            .then_expect_events(vec![AuthorizationRequestEvent::AuthorizationRequestObjectSigned {
                signed_authorization_request_object: signed_authorization_request_object(verifier_did_method),
            }]);
    }

    pub async fn verifier_did(did_method: &str) -> String {
        VERIFIER.identifier(did_method).await.unwrap()
    }

    pub fn siopv2_client_metadata(
        did_method: &str,
    ) -> ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters> {
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: vec![SubjectSyntaxType::from_str(did_method).unwrap()],
            },
        }
    }

    pub fn oid4vp_client_metadata() -> ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters> {
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            // TODO: fix this once `vp_formats` is public.
            extension: serde_json::from_value(json!({
                "vp_formats": {}
            }))
            .unwrap(),
        }
    }

    pub async fn authorization_request(response_type: &str, did_method: &str) -> GenericAuthorizationRequest {
        match response_type {
            "id_token" => GenericAuthorizationRequest::SIOPv2(Box::new(
                SIOPv2AuthorizationRequest::builder()
                    .client_id(verifier_did(did_method).await)
                    .scope(Scope::openid())
                    .redirect_uri(REDIRECT_URI.clone())
                    .response_mode("direct_post".to_string())
                    .client_metadata(siopv2_client_metadata(did_method))
                    .nonce("nonce".to_string())
                    .state("state".to_string())
                    .build()
                    .unwrap(),
            )),
            "vp_token" => GenericAuthorizationRequest::OID4VP(Box::new(
                OID4VPAuthorizationRequest::builder()
                    .client_id(verifier_did(did_method).await)
                    .client_id_scheme(ClientIdScheme::Did)
                    .scope(Scope::openid())
                    .redirect_uri(REDIRECT_URI.clone())
                    .response_mode("direct_post".to_string())
                    .presentation_definition(PRESENTATION_DEFINITION.clone())
                    .client_metadata(oid4vp_client_metadata())
                    .nonce("nonce".to_string())
                    .state("state".to_string())
                    .build()
                    .unwrap(),
            )),
            _ => unimplemented!(),
        }
    }

    pub fn form_url_encoded_authorization_request(did_method: &str) -> String {
        match did_method {
            "did:key" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY.clone(),
            "did:jwk" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK.clone(),
            "did:iota:rms" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_IOTA.clone(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    pub fn signed_authorization_request_object(did_method: &str) -> String {
        match did_method {
            "did:key" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY.clone(),
            "did:jwk" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK.clone(),
            "did:iota:rms" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_IOTA.clone(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    lazy_static! {
        pub static ref VERIFIER: Subject = futures::executor::block_on(async { Subject { secret_manager: secret_manager().await } });
        pub static ref REDIRECT_URI: url::Url = "https://my-domain.example.org/redirect".parse::<url::Url>().unwrap();
        pub static ref PRESENTATION_DEFINITION: PresentationDefinition = serde_json::from_value(json!(
            {
                "id":"Verifiable Presentation request for sign-on",
                    "input_descriptors":[
                    {
                        "id":"Request for Verifiable Credential",
                        "constraints":{
                            "fields":[
                                {
                                    "path":[
                                        "$.vc.type"
                                    ],
                                    "filter":{
                                        "type":"array",
                                        "contains":{
                                            "const":"TestCredential"
                                        }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        )).unwrap();
        static ref FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY: String = "\
        openid://?\
            client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            .to_string();
        static ref FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK: String = "\
        openid://?\
            client_id=did%3Ajwk%3AeyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            .to_string();
        static ref FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_IOTA: String = "\
        openid://?\
            client_id=did%3Aiota%3Arms%3A0x42ad588322e58b3c07aa39e4948d021ee17ecb5747915e9e1f35f028d7ecaf90&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJjbGllbnRfaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInJlZGlyZWN0X3VyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL3JlZGlyZWN0Iiwic3RhdGUiOiJzdGF0ZSIsInJlc3BvbnNlX3R5cGUiOiJpZF90b2tlbiIsInNjb3BlIjoib3BlbmlkIiwicmVzcG9uc2VfbW9kZSI6ImRpcmVjdF9wb3N0Iiwibm9uY2UiOiJub25jZSIsImNsaWVudF9tZXRhZGF0YSI6eyJzdWJqZWN0X3N5bnRheF90eXBlc19zdXBwb3J0ZWQiOlsiZGlkOmtleSJdfX0.38tCXF1QH3ihT4TgIDPToXG2EnmoRbGHRxdpLNRly8nnKPxmU4mAiroIBWA5E2SEjCpGlx_wOToymX6G0xqOBQ"
                .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKaVVVdFJVbnBoYjNBM1EyZEZkbkZXY1RoVmJHZE1SM05rUmkxU0xXaHVURVpyUzBaYWNWY3lWazR3SWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWtkc2JrczVaVkJ6T0RBeVdIaEJaMnhTVDFGNmIwZDFjbTA1VVhCMk1FbEdVRVZpWkUxRFNVeE9YMVVpZlEjMCJ9.eyJjbGllbnRfaWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWRpSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSmlVVXRSVW5waGIzQTNRMmRGZG5GV2NUaFZiR2RNUjNOa1JpMVNMV2h1VEVaclMwWmFjVmN5Vms0d0lpd2lhM1I1SWpvaVQwdFFJaXdpZUNJNklrZHNia3M1WlZCek9EQXlXSGhCWjJ4U1QxRjZiMGQxY20wNVVYQjJNRWxHVUVWaVpFMURTVXhPWDFVaWZRIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXRlIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJyZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2xpZW50X21ldGFkYXRhIjp7InN1YmplY3Rfc3ludGF4X3R5cGVzX3N1cHBvcnRlZCI6WyJkaWQ6andrIl19fQ.pgRD8qLjRn1FdKYVyY6AJpUIesYSM1Bn9UR00ZM4J22E41Vs9FwAeTOSisSseTNonZJBl3OHkj_9MBO9WnOTAg"
                .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_IOTA: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDppb3RhOnJtczoweDQyYWQ1ODgzMjJlNThiM2MwN2FhMzllNDk0OGQwMjFlZTE3ZWNiNTc0NzkxNWU5ZTFmMzVmMDI4ZDdlY2FmOTAjYlFLUVJ6YW9wN0NnRXZxVnE4VWxnTEdzZEYtUi1obkxGa0tGWnFXMlZOMCJ9.eyJjbGllbnRfaWQiOiJkaWQ6aW90YTpybXM6MHg0MmFkNTg4MzIyZTU4YjNjMDdhYTM5ZTQ5NDhkMDIxZWUxN2VjYjU3NDc5MTVlOWUxZjM1ZjAyOGQ3ZWNhZjkwIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXRlIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJyZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2xpZW50X21ldGFkYXRhIjp7InN1YmplY3Rfc3ludGF4X3R5cGVzX3N1cHBvcnRlZCI6WyJkaWQ6aW90YTpybXMiXX19.2JJOLSsMbFSKZVRPHYMGjorCJLsQE2ZV-GLQKIu86sC5VxqbQ0J37Nsrj_9U1Cz4kEU_VGYoyhOKQ7wYcJjfDA"
                .to_string();
    }
}
