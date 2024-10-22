use super::{command::AuthorizationRequestCommand, error::AuthorizationRequestError, event::AuthorizationRequestEvent};
use crate::{
    generic_oid4vc::{
        GenericAuthorizationRequest, GenericAuthorizationResponse, OID4VPAuthorizationRequest,
        SIOPv2AuthorizationRequest,
    },
    services::VerificationServices,
};
use agent_shared::{
    config::{config, get_preferred_signing_algorithm},
    verifier::Verifier,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cqrs_es::Aggregate;
use did_manager::Resolver;
use identity_credential::sd_jwt_vc::SdJwtVc;
use identity_iota::{
    core::ToJson as _,
    credential::KeyBindingJWTValidationOptions,
    did::DID as _,
    document::DIDUrlQuery,
    verification::jwk::{Jwk, JwkParams},
};
use oid4vc_core::{authorization_request::ByReference, scope::Scope};
use oid4vp::{authorization_request::ClientIdScheme, oid4vp_params::OneOrManyVpToken, Oid4vpParams};
use sd_jwt_payload_rework::{RequiredKeyBinding, Sha256Hasher};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthorizationRequest {
    pub authorization_request: Option<GenericAuthorizationRequest>,
    pub form_url_encoded_authorization_request: Option<String>,
    pub signed_authorization_request_object: Option<String>,
    pub id_token: Option<String>,
    pub vp_tokens: Option<Vec<String>>,
    pub state: Option<String>,
}

// fn validate_sd_jwt_vc(sd_jwt_vc: &SdJwtVc) -> String {}

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
                let verifier_did = verifier
                    .identifier(&default_subject_syntax_type, get_preferred_signing_algorithm())
                    .await
                    .unwrap();

                let url = &config().url;
                let request_uri = format!("{url}request/{state}").parse().unwrap();
                let redirect_uri = format!("{url}redirect").parse::<url::Url>().unwrap();

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
            VerifyAuthorizationResponse {
                // TODO: use this once `RelyingPartyManager` uses the official SIOPv2 validation logic.
                authorization_request: _,
                authorization_response,
            } => {
                let relying_party = &services.relying_party;

                match authorization_response {
                    GenericAuthorizationResponse::SIOPv2(authorization_response) => {
                        let _ = relying_party
                            .validate_response(&authorization_response)
                            .await
                            .map_err(InvalidSIOPv2AuthorizationResponse)?;

                        let id_token = authorization_response.extension.id_token.clone();

                        Ok(vec![SIOPv2AuthorizationResponseVerified {
                            id_token,
                            state: authorization_response.state,
                        }])
                    }
                    GenericAuthorizationResponse::OID4VP(oid4vp_authorization_response) => {
                        let mut vp_tokens = match &oid4vp_authorization_response.extension.oid4vp_parameters {
                            Oid4vpParams::Params {
                                vp_token: OneOrManyVpToken::One(vp_token),
                                ..
                            } => vec![vp_token.clone()],
                            Oid4vpParams::Params {
                                vp_token: OneOrManyVpToken::Many(vp_token),
                                ..
                            } => vp_token.clone(),
                            Oid4vpParams::Jwt { .. } => return Err(UnsupportedJwtParameterError),
                        };

                        for vp_token in &mut vp_tokens {
                            if let Ok(sd_jwt_vc) = vp_token.parse::<SdJwtVc>() {
                                info!("VC SD-JWT: {}", sd_jwt_vc);

                                if let Some(cnf) = &sd_jwt_vc.claims().cnf {
                                    let jwk = match cnf {
                                        RequiredKeyBinding::Jwk(jwk) => Jwk::from_params(
                                            serde_json::from_value::<JwkParams>(serde_json::json!(jwk))
                                                .map_err(|e| InvalidCnfParameterError(e.to_string()))?,
                                        ),
                                        RequiredKeyBinding::Kid(kid) => {
                                            info!("Cnf `kid` value: {kid}");

                                            let did_url = identity_iota::did::DIDUrl::parse(kid)
                                                .map_err(|e| InvalidDidUrlError(format!("Invalid DID URL: {}", e)))?;

                                            let resolver = Resolver::new().await;

                                            let document = resolver
                                                .resolve(did_url.did().as_str())
                                                .await
                                                .map_err(|e| UnsupportedDidMethodError(e.to_string()))?;

                                            let verification_method = document
                                                .resolve_method(
                                                    DIDUrlQuery::from(&did_url),
                                                    Some(identity_iota::verification::MethodScope::VerificationMethod),
                                                )
                                                .ok_or(MissingVerificationMethodError)?;

                                            verification_method
                                                .data()
                                                .public_key_jwk()
                                                .ok_or(MissingVerificationMethodKeyError)?
                                                .clone()
                                        }
                                        _ => return Err(UnsupportedCnfParameterError),
                                    };

                                    sd_jwt_vc
                                        .validate_key_binding(
                                            &Verifier,
                                            &jwk,
                                            &Sha256Hasher::new(),
                                            &KeyBindingJWTValidationOptions::default(),
                                        )
                                        .map_err(|_| InvalidKeyBindingError)?;
                                }
                                let disclosed_object = sd_jwt_vc.into_disclosed_object(&Sha256Hasher::new()).unwrap();

                                info!("Disclosed object: {:?}", disclosed_object);

                                *vp_token = URL_SAFE_NO_PAD.encode(
                                    disclosed_object
                                        .to_json_vec()
                                        .map_err(|e| InvalidDisclosedObjectError(e.to_string()))?,
                                );
                            } else {
                                let _ = relying_party
                                    .validate_response(&oid4vp_authorization_response)
                                    .await
                                    .map_err(InvalidOID4VPAuthorizationResponse)?;
                            }
                        }
                        Ok(vec![OID4VPAuthorizationResponseVerified {
                            vp_tokens,
                            state: oid4vp_authorization_response.state,
                        }])
                    }
                }
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
            SIOPv2AuthorizationResponseVerified { id_token, state } => {
                self.id_token.replace(id_token);
                self.state = state;
            }
            OID4VPAuthorizationResponseVerified { vp_tokens, state } => {
                self.vp_tokens.replace(vp_tokens);
                self.state = state;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::str::FromStr;

    use agent_secret_manager::secret_manager;
    use agent_secret_manager::service::Service as _;
    use agent_secret_manager::subject::Subject;
    use agent_shared::config::set_config;
    use agent_shared::config::SupportedDidMethod;
    use cqrs_es::test::TestFramework;
    use identity_credential::credential::Jwt;
    use identity_credential::presentation::Presentation;
    use jsonwebtoken::Algorithm;
    use lazy_static::lazy_static;
    use oid4vc_core::Subject as _;
    use oid4vc_core::{client_metadata::ClientMetadataResource, SubjectSyntaxType};
    use oid4vc_manager::managers::presentation::create_presentation_submission;
    use oid4vc_manager::ProviderManager;
    use oid4vci::VerifiableCredentialJwt;
    use oid4vp::oid4vp::AuthorizationResponseInput;
    use oid4vp::oid4vp::PresentationInputType;
    use oid4vp::PresentationDefinition;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    type AuthorizationRequestTestFramework = TestFramework<AuthorizationRequest>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_authorization_request(
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk, SupportedDidMethod::IotaRms)]
        verifier_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method.clone());

        let verification_services = VerificationServices::default();
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();
        let oid4vp_client_metadata = verification_services.oid4vp_client_metadata.clone();

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::CreateAuthorizationRequest {
                state: "state".to_string(),
                nonce: "nonce".to_string(),
                presentation_definition: None,
            })
            .then_expect_events(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(
                        authorization_request(
                            "id_token",
                            &verifier_did_method.to_string(),
                            siopv2_client_metadata,
                            oid4vp_client_metadata,
                        )
                        .await,
                    ),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        &verifier_did_method.to_string(),
                    ),
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_sign_authorization_request_object(
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk, SupportedDidMethod::IotaRms)]
        verifier_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method.clone());

        let verification_services = VerificationServices::default();
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();
        let oid4vp_client_metadata = verification_services.oid4vp_client_metadata.clone();

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(
                        authorization_request(
                            "id_token",
                            &verifier_did_method.to_string(),
                            siopv2_client_metadata,
                            oid4vp_client_metadata,
                        )
                        .await,
                    ),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        &verifier_did_method.to_string(),
                    ),
                },
            ])
            .when(AuthorizationRequestCommand::SignAuthorizationRequestObject)
            .then_expect_events(vec![AuthorizationRequestEvent::AuthorizationRequestObjectSigned {
                signed_authorization_request_object: signed_authorization_request_object(
                    &verifier_did_method.to_string(),
                ),
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_verify_authorization_response(
        // "id_token" represents the `SIOPv2` flow, and "vp_token" represents the `OID4VP` flow.
        #[values("id_token", "vp_token")] response_type: &str,
        // TODO: add `did:web`, check for other tests as well. Probably should be moved to E2E test.
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk, SupportedDidMethod::IotaRms)]
        verifier_did_method: SupportedDidMethod,
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk, SupportedDidMethod::IotaRms)]
        provider_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method.clone());

        let verification_services = VerificationServices::default();
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();
        let oid4vp_client_metadata = verification_services.oid4vp_client_metadata.clone();

        let authorization_request = authorization_request(
            response_type,
            &verifier_did_method.to_string(),
            siopv2_client_metadata,
            oid4vp_client_metadata,
        )
        .await;

        let authorization_response =
            authorization_response(&provider_did_method.to_string(), &authorization_request).await;
        let token = authorization_response.token();

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::VerifyAuthorizationResponse {
                authorization_request,
                authorization_response,
            })
            .then_expect_events(vec![match response_type {
                "id_token" => AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
                    id_token: token,
                    state: Some("state".to_string()),
                },
                "vp_token" => AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified {
                    vp_tokens: vec![token],
                    state: Some("state".to_string()),
                },
                _ => unreachable!("Invalid response type."),
            }]);
    }

    async fn authorization_response(
        did_method: &str,
        authorization_request: &GenericAuthorizationRequest,
    ) -> GenericAuthorizationResponse {
        let provider_manager = ProviderManager::new(
            Arc::new(futures::executor::block_on(async {
                Subject {
                    secret_manager: Arc::new(tokio::sync::Mutex::new(secret_manager().await)),
                }
            })),
            vec![did_method],
            vec![Algorithm::EdDSA],
        )
        .unwrap();

        let default_did_method = provider_manager.default_subject_syntax_types()[0].to_string();

        match authorization_request {
            GenericAuthorizationRequest::SIOPv2(siopv2_authorization_request) => GenericAuthorizationResponse::SIOPv2(
                provider_manager
                    .generate_response(siopv2_authorization_request, Default::default())
                    .await
                    .unwrap(),
            ),
            GenericAuthorizationRequest::OID4VP(oid4vp_authorization_request) => {
                // TODO: implement test fixture for subject and issuer instead of using the same did as verifier.
                // Fixtures can be implemented using the `rstest` crate as described here: https://docs.rs/rstest/latest/rstest/attr.fixture.html
                let issuer_did = verifier_did(&default_did_method).await;
                let subject_did = issuer_did.clone();

                // Create a new verifiable credential.
                let verifiable_credential = VerifiableCredentialJwt::builder()
                    .sub(&subject_did)
                    .iss(&issuer_did)
                    .iat(0)
                    .exp(9999999999i64)
                    .verifiable_credential(serde_json::json!({
                        "@context": [
                            "https://www.w3.org/2018/credentials/v1",
                            "https://www.w3.org/2018/credentials/examples/v1"
                        ],
                        "type": [
                            "VerifiableCredential",
                            "TestCredential"
                        ],
                        "issuanceDate": "2022-01-01T00:00:00Z",
                        "issuer": issuer_did,
                        "credentialSubject": {
                        "id": subject_did,
                        "givenName": "Ferris",
                        "familyName": "Crabman",
                        "email": "ferris.crabman@crabmail.com",
                        "birthdate": "1985-05-21"
                        }
                    }))
                    .build()
                    .unwrap();

                // Encode the verifiable credential as a JWT.
                let jwt = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkI3o2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvZXhhbXBsZXMvdjEiXSwidHlwZSI6WyJWZXJpZmlhYmxlQ3JlZGVudGlhbCIsIlRlc3RDcmVkZW50aWFsIl0sImlzc3VhbmNlRGF0ZSI6IjIwMjItMDEtMDFUMDA6MDA6MDBaIiwiaXNzdWVyIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQiLCJjcmVkZW50aWFsU3ViamVjdCI6eyJpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwiZ2l2ZW5OYW1lIjoiRmVycmlzIiwiZmFtaWx5TmFtZSI6IkNyYWJtYW4iLCJlbWFpbCI6ImZlcnJpcy5jcmFibWFuQGNyYWJtYWlsLmNvbSIsImJpcnRoZGF0ZSI6IjE5ODUtMDUtMjEifX19.6guSHngBj_QQYom3kXKmxKrHExoyW1eObBsBg8ACYn-H30YD6eub56zsWnnMzw8IznGDYAguuo3V1D37-A_vCQ".to_string();

                // Create presentation submission using the presentation definition and the verifiable credential.
                let presentation_submission = create_presentation_submission(
                    &PRESENTATION_DEFINITION,
                    &[serde_json::to_value(&verifiable_credential).unwrap()],
                )
                .unwrap();

                // Create a verifiable presentation using the JWT.
                let verifiable_presentation =
                    Presentation::builder(subject_did.parse().unwrap(), identity_core::common::Object::new())
                        .credential(Jwt::from(jwt))
                        .build()
                        .unwrap();

                GenericAuthorizationResponse::OID4VP(
                    provider_manager
                        .generate_response(
                            oid4vp_authorization_request,
                            AuthorizationResponseInput {
                                verifiable_presentation_input: vec![PresentationInputType::Unsigned(
                                    verifiable_presentation,
                                )],
                                presentation_submission,
                            },
                        )
                        .await
                        .unwrap(),
                )
            }
        }
    }

    pub async fn verifier_did(did_method: &str) -> String {
        VERIFIER.identifier(did_method, Algorithm::EdDSA).await.unwrap()
    }

    pub fn siopv2_client_metadata(
        did_method: &str,
    ) -> ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters> {
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: vec![SubjectSyntaxType::from_str(did_method).unwrap()],
                id_token_signed_response_alg: None,
            },
            other: Default::default(),
        }
    }

    pub fn oid4vp_client_metadata() -> ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters> {
        ClientMetadataResource::ClientMetadata {
            client_name: None,
            logo_uri: None,
            // TODO: fix this once `vp_formats` is public.
            extension: serde_json::from_value(json!({
                "vp_formats": {
                    "jwt_vc_json": {
                        "alg": ["EdDSA"]
                    }
                }
            }))
            .unwrap(),
            other: Default::default(),
        }
    }

    pub async fn authorization_request(
        response_type: &str,
        did_method: &str,
        siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
        oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
    ) -> GenericAuthorizationRequest {
        match response_type {
            "id_token" => GenericAuthorizationRequest::SIOPv2(Box::new(
                SIOPv2AuthorizationRequest::builder()
                    .client_id(verifier_did(did_method).await)
                    .scope(Scope::openid())
                    .redirect_uri(REDIRECT_URI.clone())
                    .response_mode("direct_post".to_string())
                    .client_metadata(siopv2_client_metadata)
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
                    .client_metadata(oid4vp_client_metadata)
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
            "did:key" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY.to_string(),
            "did:jwk" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK.to_string(),
            "did:iota:rms" => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_IOTA.to_string(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    pub fn signed_authorization_request_object(did_method: &str) -> String {
        match did_method {
            "did:key" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY.to_string(),
            "did:jwk" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK.to_string(),
            "did:iota:rms" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_IOTA.to_string(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    lazy_static! {
        pub static ref VERIFIER: Subject = futures::executor::block_on(async {
            Subject {
                secret_manager: Arc::new(tokio::sync::Mutex::new(secret_manager().await)),
            }
        });
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
        ))
        .unwrap();
    }
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY: &str = "\
        openid://?\
            client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK: &str = "\
        openid://?\
            client_id=did%3Ajwk%3AeyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_IOTA: &str = "\
        openid://?\
            client_id=did%3Aiota%3Arms%3A0x42ad588322e58b3c07aa39e4948d021ee17ecb5747915e9e1f35f028d7ecaf90&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJjbGllbnRfaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInJlZGlyZWN0X3VyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL3JlZGlyZWN0Iiwic3RhdGUiOiJzdGF0ZSIsInJlc3BvbnNlX3R5cGUiOiJpZF90b2tlbiIsInNjb3BlIjoib3BlbmlkIiwicmVzcG9uc2VfbW9kZSI6ImRpcmVjdF9wb3N0Iiwibm9uY2UiOiJub25jZSIsImNsaWVudF9tZXRhZGF0YSI6eyJjbGllbnRfbmFtZSI6IlVuaUNvcmUiLCJsb2dvX3VyaSI6Imh0dHBzOi8vaW1waWVyY2UuY29tL2ltYWdlcy9mYXZpY29uL2FwcGxlLXRvdWNoLWljb24ucG5nIiwic3ViamVjdF9zeW50YXhfdHlwZXNfc3VwcG9ydGVkIjpbImRpZDpqd2siLCJkaWQ6a2V5IiwiZGlkOmlvdGE6cm1zIl0sImlkX3Rva2VuX3NpZ25lZF9yZXNwb25zZV9hbGciOiJFZERTQSIsImlkX3Rva2VuX3NpZ25pbmdfYWxnX3ZhbHVlc19zdXBwb3J0ZWQiOlsiRWREU0EiXX19.bSJic_ZsIygLNYCi2cZBeAncGw68RNN64-nTOC6Mi09yF1NXuPJE3J5qWupjycVLf7LscYKDCjO50kvGf4fPDw";
    const SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKaVVVdFJVbnBoYjNBM1EyZEZkbkZXY1RoVmJHZE1SM05rUmkxU0xXaHVURVpyUzBaYWNWY3lWazR3SWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWtkc2JrczVaVkJ6T0RBeVdIaEJaMnhTVDFGNmIwZDFjbTA1VVhCMk1FbEdVRVZpWkUxRFNVeE9YMVVpZlEjMCJ9.eyJjbGllbnRfaWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWRpSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSmlVVXRSVW5waGIzQTNRMmRGZG5GV2NUaFZiR2RNUjNOa1JpMVNMV2h1VEVaclMwWmFjVmN5Vms0d0lpd2lhM1I1SWpvaVQwdFFJaXdpZUNJNklrZHNia3M1WlZCek9EQXlXSGhCWjJ4U1QxRjZiMGQxY20wNVVYQjJNRWxHVUVWaVpFMURTVXhPWDFVaWZRIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXRlIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJyZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2xpZW50X21ldGFkYXRhIjp7ImNsaWVudF9uYW1lIjoiVW5pQ29yZSIsImxvZ29fdXJpIjoiaHR0cHM6Ly9pbXBpZXJjZS5jb20vaW1hZ2VzL2Zhdmljb24vYXBwbGUtdG91Y2gtaWNvbi5wbmciLCJzdWJqZWN0X3N5bnRheF90eXBlc19zdXBwb3J0ZWQiOlsiZGlkOmp3ayIsImRpZDprZXkiLCJkaWQ6aW90YTpybXMiXSwiaWRfdG9rZW5fc2lnbmVkX3Jlc3BvbnNlX2FsZyI6IkVkRFNBIiwiaWRfdG9rZW5fc2lnbmluZ19hbGdfdmFsdWVzX3N1cHBvcnRlZCI6WyJFZERTQSJdfX0.8Qj3u6rC5Qb0W54duip_HeJdp9It104Im8BKNR4H6Pw5AY6U826q-GBO618TLwavm2I20ehA8XWGYoOBzPyGDQ";
    const SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_IOTA: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDppb3RhOnJtczoweDQyYWQ1ODgzMjJlNThiM2MwN2FhMzllNDk0OGQwMjFlZTE3ZWNiNTc0NzkxNWU5ZTFmMzVmMDI4ZDdlY2FmOTAjYlFLUVJ6YW9wN0NnRXZxVnE4VWxnTEdzZEYtUi1obkxGa0tGWnFXMlZOMCJ9.eyJjbGllbnRfaWQiOiJkaWQ6aW90YTpybXM6MHg0MmFkNTg4MzIyZTU4YjNjMDdhYTM5ZTQ5NDhkMDIxZWUxN2VjYjU3NDc5MTVlOWUxZjM1ZjAyOGQ3ZWNhZjkwIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXRlIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJyZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2xpZW50X21ldGFkYXRhIjp7ImNsaWVudF9uYW1lIjoiVW5pQ29yZSIsImxvZ29fdXJpIjoiaHR0cHM6Ly9pbXBpZXJjZS5jb20vaW1hZ2VzL2Zhdmljb24vYXBwbGUtdG91Y2gtaWNvbi5wbmciLCJzdWJqZWN0X3N5bnRheF90eXBlc19zdXBwb3J0ZWQiOlsiZGlkOmp3ayIsImRpZDprZXkiLCJkaWQ6aW90YTpybXMiXSwiaWRfdG9rZW5fc2lnbmVkX3Jlc3BvbnNlX2FsZyI6IkVkRFNBIiwiaWRfdG9rZW5fc2lnbmluZ19hbGdfdmFsdWVzX3N1cHBvcnRlZCI6WyJFZERTQSJdfX0.TGQ_9RQYwltCjE8mRVG1CFveoQjWH9Xf55pm8TcYLkOmUitHeK_PKwwO16vWXHfgLeAVe7Y5b98hKCAupZ6FBg";
}
