use std::sync::Arc;

use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::{
    authorization_request::{Body, ByReference, Object},
    scope::Scope,
};
use oid4vp::{authorization_request::ClientIdScheme, oid4vp::OID4VP, PresentationDefinition};
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;
use tracing::info;

use crate::services::VerificationServices;

use super::{command::AuthorizationRequestCommand, error::AuthorizationRequestError, event::AuthorizationRequestEvent};

pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;
pub type OID4VPAuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<OID4VP>>;

// TODO: come up with a better name for this type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum GenericAuthorizationRequest {
    SIOPv2(SIOPv2AuthorizationRequest),
    OID4VP(OID4VPAuthorizationRequest),
}

impl GenericAuthorizationRequest {
    pub fn as_siopv2_authorization_request(&self) -> Option<&SIOPv2AuthorizationRequest> {
        match self {
            GenericAuthorizationRequest::SIOPv2(authorization_request) => Some(authorization_request),
            _ => None,
        }
    }

    pub fn as_oid4vp_authorization_request(&self) -> Option<&OID4VPAuthorizationRequest> {
        match self {
            GenericAuthorizationRequest::OID4VP(authorization_request) => Some(authorization_request),
            _ => None,
        }
    }

    pub fn client_id(&self) -> String {
        match self {
            GenericAuthorizationRequest::SIOPv2(authorization_request) => {
                authorization_request.body.client_id().clone()
            }
            GenericAuthorizationRequest::OID4VP(authorization_request) => {
                authorization_request.body.client_id().clone()
            }
        }
    }
}

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
        use AuthorizationRequestEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationRequest {
                state,
                nonce,
                presentation_definition_id,
            } => {
                let default_subject_syntax_type = services.relying_party.default_subject_syntax_type().to_string();
                let verifier = &services.verifier;
                let verifier_did = verifier.identifier(&default_subject_syntax_type).unwrap();

                let url = config!("url").unwrap();
                let request_uri = format!("{url}/request/{state}").parse().unwrap();
                let redirect_uri = format!("{url}/redirect").parse::<url::Url>().unwrap();

                let authorization_request =
                    Box::new(if let Some(presentation_definition_id) = presentation_definition_id {
                        // TODO: Fix this path
                        let presentation_definition: PresentationDefinition = match std::fs::File::open(format!(
                            "../agent_verification/presentation_definitions/{presentation_definition_id}.json"
                        )) {
                            // FIX THISS
                            Ok(presentation_definition) => serde_json::from_reader(presentation_definition).unwrap(),
                            // FIX THISS
                            Err(hello) => panic!("{}", hello),
                        };

                        GenericAuthorizationRequest::OID4VP(
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
                                // FIX THISS
                                .unwrap(),
                        )
                    } else {
                        GenericAuthorizationRequest::SIOPv2(
                            SIOPv2AuthorizationRequest::builder()
                                .client_id(verifier_did.clone())
                                .scope(Scope::openid())
                                .redirect_uri(redirect_uri)
                                .response_mode("direct_post".to_string())
                                .client_metadata(services.siopv2_client_metadata.clone())
                                .state(state)
                                .nonce(nonce)
                                .build()
                                // FIX THISS
                                .unwrap(),
                        )
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

                // FIX THISS
                let temp = self.authorization_request.as_ref().unwrap();

                let signed_authorization_request_object =
                    if let Some(siopv2_authorization_request) = temp.as_siopv2_authorization_request() {
                        relying_party.encode(siopv2_authorization_request)
                    } else if let Some(oid4vp_authorization_request) = temp.as_oid4vp_authorization_request() {
                        relying_party.encode(oid4vp_authorization_request)
                    } else {
                        todo!()
                    }
                    // FIX THISS
                    .unwrap();

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
                self.authorization_request = Some(*authorization_request);
            }
            FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request,
            } => {
                self.form_url_encoded_authorization_request = Some(form_url_encoded_authorization_request);
            }
            AuthorizationRequestObjectSigned {
                signed_authorization_request_object,
            } => {
                self.signed_authorization_request_object = Some(signed_authorization_request_object);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use agent_secret_manager::secret_manager;
    use cqrs_es::test::TestFramework;
    use did_manager::SecretManager;
    use lazy_static::lazy_static;
    use oid4vc_core::Subject;
    use oid4vc_core::{client_metadata::ClientMetadataResource, DidMethod, SubjectSyntaxType};
    use rstest::rstest;
    use serde_json::json;

    use crate::services::test_utils::test_verification_services;

    use super::*;

    type AuthorizationRequestTestFramework = TestFramework<AuthorizationRequest>;

    #[rstest]
    #[serial_test::serial]
    fn test_create_authorization_request(#[values("did:key", "did:jwk")] verifier_did_method: &str) {
        let verification_services = test_verification_services(verifier_did_method);

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::CreateAuthorizationRequest {
                state: "state".to_string(),
                nonce: "nonce".to_string(),
                presentation_definition_id: None,
            })
            .then_expect_events(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(authorization_request("id_token", verifier_did_method)),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(verifier_did_method),
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    fn test_sign_authorization_request_object(#[values("did:key", "did:jwk")] verifier_did_method: &str) {
        let verification_services = test_verification_services(verifier_did_method);

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(authorization_request("id_token", verifier_did_method)),
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

    pub fn verifier_did(did_method: &str) -> String {
        VERIFIER.identifier(did_method).unwrap()
    }

    pub fn siopv2_client_metadata(
        did_method: &str,
    ) -> ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters> {
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: vec![SubjectSyntaxType::Did(DidMethod::from_str(did_method).unwrap())],
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

    pub fn authorization_request(response_type: &str, did_method: &str) -> GenericAuthorizationRequest {
        match response_type {
            "id_token" => GenericAuthorizationRequest::SIOPv2(
                SIOPv2AuthorizationRequest::builder()
                    .client_id(verifier_did(did_method))
                    .scope(Scope::openid())
                    .redirect_uri(REDIRECT_URI.clone())
                    .response_mode("direct_post".to_string())
                    .client_metadata(siopv2_client_metadata(did_method))
                    .nonce("nonce".to_string())
                    .state("state".to_string())
                    .build()
                    .unwrap(),
            ),
            "vp_token" => GenericAuthorizationRequest::OID4VP(
                OID4VPAuthorizationRequest::builder()
                    .client_id(verifier_did(did_method))
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
            ),
            _ => unimplemented!(),
        }
    }

    pub fn form_url_encoded_authorization_request(did_method: &str) -> String {
        match did_method {
            "did:key" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY.clone(),
            "did:jwk" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK.clone(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    pub fn signed_authorization_request_object(did_method: &str) -> String {
        match did_method {
            "did:key" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY.clone(),
            "did:jwk" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK.clone(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    lazy_static! {
        pub static ref VERIFIER: SecretManager = futures::executor::block_on(async { secret_manager().await });
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
            client_id=did%3Akey%3Az6MkiieyoLMSVsJAZv7Jje5wWSkDEymUgkyF8kbcrjZpX3qd&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            .to_string();
        static ref FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK: String = "\
        openid://?\
            client_id=did%3Ajwk%3AeyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJhSHEtMFBJZjZfbGpMaHl4NFc4Nkd2aXFiLTY3MU9BSTY3RTZ2WHBaYzdRIiwia3R5IjoiT0tQIiwieCI6IlAyQmtZUzZ6NFVIbXN4bjZGWDFvSHN5eDdlaVVTRkVNSjFEX1JDOE0wLXcifQ&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate"
            .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2lp\
             ZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkI3o2TWtp\
             aWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCJ9.eyJ\
             jbGllbnRfaWQiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0R\
             FeW1VZ2t5RjhrYmNyalpwWDNxZCIsInJlZGlyZWN0X3VyaSI6Imh0dHBzOi8vbXk\
             tZG9tYWluLmV4YW1wbGUub3JnL3JlZGlyZWN0Iiwic3RhdGUiOiJzdGF0ZSIsInJ\
             lc3BvbnNlX3R5cGUiOiJpZF90b2tlbiIsInNjb3BlIjoib3BlbmlkIiwicmVzcG9\
             uc2VfbW9kZSI6ImRpcmVjdF9wb3N0Iiwibm9uY2UiOiJub25jZSIsImNsaWVudF9\
             tZXRhZGF0YSI6eyJzdWJqZWN0X3N5bnRheF90eXBlc19zdXBwb3J0ZWQiOlsiZGl\
             kOmtleSJdfX0.Q9SLE69k4qk1L72yHq3PlY0YyZm1m9do7Wlu3HjzjbHnKnzB6gT\
             5ZfG04krgRf99CgyVeDh9DKnUGrHBUQN2CA"
                .to_string();
        static ref SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK: String =
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDpqd2s6ZXlKaGJH\
             Y2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKaFNI\
             RXRNRkJKWmpaZmJHcE1hSGw0TkZjNE5rZDJhWEZpTFRZM01VOUJTVFkzUlRaMldI\
             QmFZemRSSWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWxBeVFtdFpVelo2TkZWSWJY\
             TjRialpHV0RGdlNITjVlRGRsYVZWVFJrVk5TakZFWDFKRE9FMHdMWGNpZlEjMCJ9\
             .eyJjbGllbnRfaWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWR\
             pSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSmhTSEV0TUZCSlpqWmZiR3BNYUhsNE5\
             GYzROa2QyYVhGaUxUWTNNVTlCU1RZM1JUWjJXSEJhWXpkUklpd2lhM1I1SWpvaVQ\
             wdFFJaXdpZUNJNklsQXlRbXRaVXpaNk5GVkliWE40YmpaR1dERnZTSE41ZURkbGF\
             WVlRSa1ZOU2pGRVgxSkRPRTB3TFhjaWZRIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM\
             6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXR\
             lIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJ\
             yZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2x\
             pZW50X21ldGFkYXRhIjp7InN1YmplY3Rfc3ludGF4X3R5cGVzX3N1cHBvcnRlZCI\
             6WyJkaWQ6andrIl19fQ.Zd-zz7WwTpitagNWUBUAV-PmZ2SP8ceEaLSh4jY-Q2Tw\
             W3NsoNGvTbd2xXy1BG8NP3xW3sqmWzObcc0UN6YqCQ"
                .to_string();
    }
}
