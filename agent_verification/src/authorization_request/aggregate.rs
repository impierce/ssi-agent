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
    credential_status_checker::CredentialStatusChecker,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::{authorization_request::ByReference, scope::Scope, verifier::SignatureVerifier};
use oid4vc_core::{client_metadata::ClientMetadataResource, Subject as _};
use oid4vp::token::vp_token_validator::VpTokenValidator;
use oid4vp::{authorization_request::ClientId, token::vp_token_validator::DecodedVpToken};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthorizationRequest {
    pub authorization_request: Option<GenericAuthorizationRequest>,
    pub form_url_encoded_authorization_request: Option<String>,
    pub signed_authorization_request_object: Option<String>,
    pub id_token: Option<String>,
    pub vp_token: Option<DecodedVpToken>,
    pub state: Option<String>,
    pub validated: bool,
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
                dcql_query,
            } => {
                let default_subject_syntax_type = services.relying_party.default_subject_syntax_type().to_string();
                let verifier = &services.verifier;
                let verifier_did = verifier
                    .identifier(&default_subject_syntax_type, get_preferred_signing_algorithm())
                    .await
                    .unwrap();

                let mut request_uri = config().request_uri.clone();
                if let Ok(mut path_segments) = request_uri.path_segments_mut() {
                    path_segments.pop_if_empty();
                    path_segments.push(&state);
                }

                let redirect_uri = config().redirect_uri.clone();
                let client_id: String;

                let authorization_request = Box::new(if let Some(dcql_query) = dcql_query {
                    let mut oid4vp_client_metadata = services.oid4vp_client_metadata.clone();
                    if let ClientMetadataResource::ClientMetadata {
                        ref mut client_name,
                        ref mut logo_uri,
                        ref mut other,
                        ..
                    } = oid4vp_client_metadata
                    {
                        // TODO: remove this once the Identity Bounded Context is the single source of truth for display data.
                        // This is a temporary workaround to ensure the credential issuer metadata has the correct display information.
                        *client_name = Some(config().display.first().map(|d| d.name.clone()).unwrap_or_default());
                        *logo_uri = config()
                            .display
                            .first()
                            .and_then(|d| d.logo.as_ref().and_then(|l| l.uri.clone()));

                        if let Some(description) = config().display.first().and_then(|d| d.description.clone()) {
                            other.insert("description".to_string(), description.into());
                        }
                    }

                    client_id = format!("decentralized_identifier:{}", verifier_did);

                    GenericAuthorizationRequest::OID4VP(Box::new(
                        OID4VPAuthorizationRequest::builder()
                            .dcql_query(dcql_query)
                            .client_id(ClientId::from_str(&client_id).unwrap())
                            .scope(Scope::openid())
                            .response_uri(redirect_uri)
                            .response_mode("direct_post".to_string())
                            .client_metadata(oid4vp_client_metadata)
                            .state(state)
                            .nonce(nonce)
                            .build()
                            .map_err(AuthorizationRequestBuilderError)?,
                    ))
                } else {
                    let mut siopv2_client_metadata = services.siopv2_client_metadata.clone();
                    if let ClientMetadataResource::ClientMetadata {
                        ref mut client_name,
                        ref mut logo_uri,
                        ref mut other,
                        ..
                    } = siopv2_client_metadata
                    {
                        // TODO: remove this once the Identity Bounded Context is the single source of truth for display data.
                        // This is a temporary workaround to ensure the credential issuer metadata has the correct display information.
                        *client_name = Some(config().display.first().map(|d| d.name.clone()).unwrap_or_default());
                        *logo_uri = config()
                            .display
                            .first()
                            .and_then(|d| d.logo.as_ref().and_then(|l| l.uri.clone()));

                        if let Some(description) = config().display.first().and_then(|d| d.description.clone()) {
                            other.insert("description".to_string(), description.into());
                        }
                    }

                    client_id = verifier_did;

                    GenericAuthorizationRequest::SIOPv2(Box::new(
                        SIOPv2AuthorizationRequest::builder()
                            .client_id(client_id.clone())
                            .scope(Scope::openid())
                            .redirect_uri(redirect_uri)
                            .response_mode("direct_post".to_string())
                            .client_metadata(siopv2_client_metadata)
                            .state(state)
                            .nonce(nonce)
                            .build()
                            .map_err(AuthorizationRequestBuilderError)?,
                    ))
                });

                let form_url_encoded_authorization_request = oid4vc_core::authorization_request::AuthorizationRequest {
                    custom_url_scheme: "openid".to_string(),
                    body: ByReference { client_id, request_uri },
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
            VerifyAuthorizationResponse { authorization_response } => {
                let relying_party = &services.relying_party;

                if self.validated {
                    return Err(AuthorizationRequestError::InvalidOID4VPAuthorizationResponse(
                        anyhow::anyhow!("Nonce has already been validated."),
                    ));
                }

                match authorization_response {
                    GenericAuthorizationResponse::SIOPv2(authorization_response) => {
                        #[allow(deprecated)]
                        let _ = relying_party
                            .validate_response(&authorization_response)
                            .await
                            .map_err(InvalidSIOPv2AuthorizationResponse)?;

                        let id_token = authorization_response.extension.id_token.clone();

                        Ok(vec![SIOPv2AuthorizationResponseVerified {
                            id_token,
                            state: authorization_response.state,
                            validated: true,
                        }])
                    }
                    GenericAuthorizationResponse::OID4VP(oid4vp_authorization_response) => {
                        if let GenericAuthorizationRequest::OID4VP(authorization_request) =
                            self.authorization_request.as_ref().ok_or(MissingAuthorizationRequest)?
                        {
                            let dcql_query = &authorization_request.body.extension.dcql_query;

                            let vp_token = &oid4vp_authorization_response.extension.vp_token;

                            let nonce = &authorization_request.body.extension.nonce;
                            let client_id = &authorization_request.body.client_id;

                            let decoded_vp_token = VpTokenValidator::new(
                                &SignatureVerifier,
                                services.verifier.as_ref(),
                                &services.credential_status_checker,
                            )
                            .validate_vp_token(dcql_query, vp_token, client_id, Some(nonce))
                            .await
                            .map_err(|e| InvalidOID4VPAuthorizationResponse(e.into()))?;

                            Ok(vec![OID4VPAuthorizationResponseVerified {
                                vp_token: decoded_vp_token,
                                state: oid4vp_authorization_response.state,
                                validated: true,
                            }])
                        } else {
                            Err(MissingAuthorizationRequest)
                        }
                    }
                }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationRequestEvent::*;

        debug!("Applying event: {:?}", event);

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
            SIOPv2AuthorizationResponseVerified {
                id_token,
                state,
                validated,
            } => {
                self.id_token.replace(id_token);
                self.state = state;
                self.validated = validated;
            }
            OID4VPAuthorizationResponseVerified {
                vp_token,
                state,
                validated,
            } => {
                self.vp_token.replace(vp_token);
                self.state = state;
                self.validated = validated;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {

    use agent_secret_manager::service::Service as _;
    use agent_secret_manager::subject::Subject;
    use agent_shared::config::set_config;
    use agent_shared::config::SupportedDidMethod;
    use chrono::DateTime;
    use chrono::{Duration, Utc};
    use cqrs_es::test::TestFramework;
    use did_key::{generate, Ed25519KeyPair};
    use identity_credential::{credential::Jwt, presentation::Presentation};
    use jsonwebtoken::Algorithm;
    use jsonwebtoken::Header;
    use lazy_static::lazy_static;
    use oid4vc_core::claim_path_pointer::{ClaimPathElement, ClaimPathPointer};
    use oid4vc_core::client_metadata::ClientMetadataResource;
    use oid4vc_core::jwt;
    use oid4vc_core::types::string_or_object::StringOrObject;
    use oid4vc_core::Subject as _;
    use oid4vc_manager::methods::key_method::KeySubject;
    use oid4vc_manager::ProviderManager;
    use oid4vci::VerifiableCredentialJwt;
    use oid4vp::authorization_request::ClientId;
    use oid4vp::dcql::dcql_query::{ClaimQuery, CredentialQuery, CredentialQueryId, DcqlQuery, Format, MetaTypes};
    use oid4vp::token::verifiable_presentation_jwt::VerifiablePresentationJwt;
    use oid4vp::token::vp_token::Presentations;
    use oid4vp::token::vp_token::VpToken;
    use oid4vp::token::vp_token_builder::VpTokenBuilder;
    use oid4vp::token::vp_token_validator::DecodedVpTokenBuilder;
    use rstest::fixture;
    use rstest::rstest;
    use std::str::FromStr;
    use std::sync::Arc;

    use super::*;

    type AuthorizationRequestTestFramework = TestFramework<AuthorizationRequest>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_authorization_request(
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] verifier_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method);

        let verification_services = VerificationServices::default().await;
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();

        AuthorizationRequestTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(AuthorizationRequestCommand::CreateAuthorizationRequest {
                state: "state".to_string(),
                nonce: "nonce".to_string(),
                dcql_query: None,
            })
            .then_expect_events(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(GenericAuthorizationRequest::SIOPv2(Box::new(
                        siopv2_authorization_request(&verifier_did_method.to_string(), siopv2_client_metadata).await,
                    ))),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        "id_token",
                        &verifier_did_method.to_string(),
                    ),
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_sign_authorization_request_object(
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] verifier_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method);

        let verification_services = VerificationServices::default().await;
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(GenericAuthorizationRequest::SIOPv2(Box::new(
                        siopv2_authorization_request(&verifier_did_method.to_string(), siopv2_client_metadata).await,
                    ))),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        "id_token",
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
    async fn test_verify_siopv2_authorization_response(
        // TODO: add `did:web`, check for other tests as well. Probably should be moved to E2E test.
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] verifier_did_method: SupportedDidMethod,
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] provider_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method);

        let verification_services = VerificationServices::default().await;
        let siopv2_client_metadata = verification_services.siopv2_client_metadata.clone();

        let authorization_request = GenericAuthorizationRequest::SIOPv2(Box::new(
            siopv2_authorization_request(&verifier_did_method.to_string(), siopv2_client_metadata.clone()).await,
        ));

        let authorization_response =
            authorization_response(&provider_did_method.to_string(), &authorization_request, "nonce").await;

        let id_token = match &authorization_response {
            GenericAuthorizationResponse::SIOPv2(siopv2_response) => siopv2_response.extension.id_token.clone(),
            _ => panic!("Expected SIOPv2 authorization response."),
        };

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(GenericAuthorizationRequest::SIOPv2(Box::new(
                        siopv2_authorization_request(&verifier_did_method.to_string(), siopv2_client_metadata).await,
                    ))),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        "id_token",
                        &verifier_did_method.to_string(),
                    ),
                },
            ])
            .when(AuthorizationRequestCommand::VerifyAuthorizationResponse { authorization_response })
            .then_expect_events(vec![AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
                id_token,
                state: Some("state".to_string()),
                validated: true,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_verify_oid4vp_authorization_response(
        // TODO: add `did:web`, check for other tests as well. Probably should be moved to E2E test.
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] verifier_did_method: SupportedDidMethod,
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] provider_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method);

        let verification_services = VerificationServices::default().await;
        let oid4vp_client_metadata = verification_services.oid4vp_client_metadata.clone();

        let authorization_request =
            oid4vp_authorization_request(&verifier_did_method.to_string(), oid4vp_client_metadata.clone()).await;

        let authorization_request = GenericAuthorizationRequest::OID4VP(Box::new(authorization_request));

        let authorization_response =
            authorization_response(&provider_did_method.to_string(), &authorization_request, "nonce").await;

        let decoded_vp_token = DecodedVpTokenBuilder::new()
            .insert(
                CredentialQueryId::try_new("CredentialQuery").unwrap(),
                vec![serde_json::json!({
                    "@context": ["https://www.w3.org/2018/credentials/v1"],
                    "type": ["VerifiableCredential", "PersonalInformation"],
                    "issuanceDate": issuance_date(),
                    "issuer": "did:key:z6MkmYc7aJCBhejDpY9nYS5RMWniergAJBAzomtrqhqG2Ah5",
                    "credentialSubject": {
                        "id": "did:key:z6MkmDhE9TjaaME2ApzpWx7g2xZE3zivnEJdZT8avPUBZjuE",
                        "givenName": "Ferris",
                        "familyName": "Crabman",
                        "email": "ferris.crabman@crabmail.com",
                        "birthdate": "1985-05-21"
                    },
                    "expirationDate": expiration_date(),
                })
                .as_object()
                .unwrap()
                .clone()],
            )
            .unwrap()
            .build();

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(GenericAuthorizationRequest::OID4VP(Box::new(
                        oid4vp_authorization_request(&verifier_did_method.to_string(), oid4vp_client_metadata).await,
                    ))),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        "vp_token",
                        &verifier_did_method.to_string(),
                    ),
                },
            ])
            .when(AuthorizationRequestCommand::VerifyAuthorizationResponse { authorization_response })
            .then_expect_events(vec![AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified {
                vp_token: decoded_vp_token,
                state: Some("state".to_string()),
                validated: true,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_verify_oid4vp_authorization_response_nonce_mismatch(
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] verifier_did_method: SupportedDidMethod,
        #[values(SupportedDidMethod::Key, SupportedDidMethod::Jwk)] provider_did_method: SupportedDidMethod,
    ) {
        set_config().set_preferred_did_method(verifier_did_method);

        let verification_services = VerificationServices::default().await;
        let oid4vp_client_metadata = verification_services.oid4vp_client_metadata.clone();

        let authorization_request = GenericAuthorizationRequest::OID4VP(Box::new(
            oid4vp_authorization_request(&verifier_did_method.to_string(), oid4vp_client_metadata.clone()).await,
        ));

        let authorization_response = authorization_response(
            &provider_did_method.to_string(),
            &authorization_request,
            "mismatched-nonce",
        )
        .await;

        AuthorizationRequestTestFramework::with(verification_services)
            .given(vec![
                AuthorizationRequestEvent::AuthorizationRequestCreated {
                    authorization_request: Box::new(GenericAuthorizationRequest::OID4VP(Box::new(
                        oid4vp_authorization_request(&verifier_did_method.to_string(), oid4vp_client_metadata).await,
                    ))),
                },
                AuthorizationRequestEvent::FormUrlEncodedAuthorizationRequestCreated {
                    form_url_encoded_authorization_request: form_url_encoded_authorization_request(
                        "vp_token",
                        &verifier_did_method.to_string(),
                    ),
                },
            ])
            .when(AuthorizationRequestCommand::VerifyAuthorizationResponse {
                authorization_response,
            })
            .then_expect_error_message("Invalid OID4VP Authorization Response: Invalid nonce: expected Some(\"nonce\"), found Some(\"mismatched-nonce\")");
    }

    async fn authorization_response(
        provider_did_method: &str,
        authorization_request: &GenericAuthorizationRequest,
        nonce: &str,
    ) -> GenericAuthorizationResponse {
        let provider_manager = ProviderManager::new(
            Arc::new(Subject::test_subject().await),
            vec![provider_did_method],
            vec![Algorithm::ES256],
        )
        .unwrap();

        match authorization_request {
            GenericAuthorizationRequest::SIOPv2(siopv2_authorization_request) => GenericAuthorizationResponse::SIOPv2(
                provider_manager
                    .generate_response(siopv2_authorization_request, Default::default())
                    .await
                    .unwrap(),
            ),
            GenericAuthorizationRequest::OID4VP(oid4vp_authorization_request) => {
                let audience = oid4vp_authorization_request.body.client_id.to_string();
                let vp_token = create_simple_vp_token(provider_did_method, nonce, audience).await;

                GenericAuthorizationResponse::OID4VP(
                    provider_manager
                        .generate_response(oid4vp_authorization_request, vp_token)
                        .await
                        .unwrap(),
                )
            }
        }
    }

    pub async fn verifier_did(did_method: &str) -> String {
        Subject::test_subject()
            .await
            .identifier(did_method, Algorithm::EdDSA)
            .await
            .unwrap()
    }

    pub async fn siopv2_authorization_request(
        did_method: &str,
        siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
    ) -> SIOPv2AuthorizationRequest {
        SIOPv2AuthorizationRequest::builder()
            .client_id(verifier_did(did_method).await)
            .scope(Scope::openid())
            .redirect_uri(REDIRECT_URI.clone())
            .response_mode("direct_post".to_string())
            .client_metadata(siopv2_client_metadata)
            .nonce("nonce".to_string())
            .state("state".to_string())
            .build()
            .unwrap()
    }

    pub async fn oid4vp_authorization_request(
        did_method: &str,
        oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
    ) -> OID4VPAuthorizationRequest {
        OID4VPAuthorizationRequest::builder()
            .dcql_query(DCQL_QUERY.clone())
            .client_id(
                ClientId::from_str(&format!("decentralized_identifier:{}", verifier_did(did_method).await)).unwrap(),
            )
            .scope(Scope::openid())
            .response_uri(RESPONSE_URI.clone())
            .response_mode("direct_post".to_string())
            .client_metadata(oid4vp_client_metadata)
            .nonce("nonce".to_string())
            .state("state".to_string())
            .build()
            .unwrap()
    }

    async fn create_simple_vp_token(provider_did_method: &str, nonce: &str, audience: String) -> VpToken {
        let issuer = Arc::new(KeySubject::from_keypair(
            generate::<Ed25519KeyPair>(Some("test-issuer-key.................".as_bytes())),
            None,
        ));
        let issuer_did = issuer.identifier(provider_did_method, Algorithm::EdDSA).await.unwrap();

        let subject = Arc::new(KeySubject::from_keypair(
            generate::<Ed25519KeyPair>(Some("test-subject-key................".as_bytes())),
            None,
        ));
        let subject_did = subject.identifier(provider_did_method, Algorithm::EdDSA).await.unwrap();

        let issuance_date = issuance_date();

        let verifiable_credential = VerifiableCredentialJwt::builder()
            .sub(&subject_did)
            .iss(&issuer_did)
            .iat(issuance_date.timestamp())
            .exp(expiration_date().timestamp())
            .verifiable_credential(serde_json::json!({
                "@context": ["https://www.w3.org/2018/credentials/v1"],
                "type": ["VerifiableCredential", "PersonalInformation"],
                "issuanceDate": issuance_date,
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

        // Encode as JWT with proper headers
        let vc_jwt = jwt::encode(
            issuer.clone(),
            Header {
                alg: Algorithm::EdDSA,
                ..Default::default()
            },
            &verifiable_credential,
            provider_did_method,
        )
        .await
        .unwrap();

        let verifiable_presentation_jwt =
            Presentation::builder(subject_did.parse().unwrap(), identity_core::common::Object::new())
                .credential(Jwt::from(vc_jwt))
                .build()
                .unwrap();

        let vp_jwt_claims = VerifiablePresentationJwt::builder()
            .iss(subject_did.clone())
            .sub(subject_did)
            .aud(audience)
            .nonce(nonce.to_string())
            .exp((Utc::now() + Duration::minutes(10)).timestamp())
            .iat(Utc::now().timestamp())
            .verifiable_presentation(verifiable_presentation_jwt)
            .build()
            .unwrap();

        let vp_jwt = jwt::encode(
            subject.clone(),
            Header {
                alg: Algorithm::EdDSA,
                ..Default::default()
            },
            &vp_jwt_claims,
            provider_did_method,
        )
        .await
        .unwrap();

        let presentations = Presentations::try_new(vec![StringOrObject::from(vp_jwt)]).unwrap();

        VpTokenBuilder::new()
            .add_presentations(
                CredentialQueryId::try_new("CredentialQuery".to_string()).unwrap(),
                presentations,
            )
            .build()
            .unwrap()
    }

    pub fn form_url_encoded_authorization_request(response_type: &str, did_method: &str) -> String {
        match (response_type, did_method) {
            ("id_token", "did:key") => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY_SIOPV2.to_string(),
            ("id_token", "did:jwk") => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK_SIOPV2.to_string(),
            ("vp_token", "did:key") => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY_OID4VP.to_string(),
            ("vp_token", "did:jwk") => FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK_OID4VP.to_string(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    pub fn signed_authorization_request_object(did_method: &str) -> String {
        match did_method {
            "did:key" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY.to_string(),
            "did:jwk" => SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK.to_string(),
            _ => unimplemented!("Unknown DID method: {}", did_method),
        }
    }

    #[fixture]
    fn issuance_date() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1773187205, 0).unwrap()
    }

    #[fixture]
    fn expiration_date() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(9999999999i64, 0).unwrap()
    }

    lazy_static! {
        pub static ref REDIRECT_URI: url::Url = "https://my-domain.example.org/redirect".parse::<url::Url>().unwrap();
        pub static ref RESPONSE_URI: url::Url = "https://my-domain.example.org/redirect".parse::<url::Url>().unwrap();
        pub static ref DCQL_QUERY: DcqlQuery = DcqlQuery {
            credentials: vec![CredentialQuery {
                id: CredentialQueryId::try_new("CredentialQuery".to_string()).unwrap(),
                format: Format::JwtVcJson,
                multiple: None,
                meta: MetaTypes::W3CFormatMeta {
                    type_values: vec![
                        vec!["VerifiableCredential".to_string()],
                        vec!["PersonalInformation".to_string()]
                    ]
                },
                trusted_authorities: None,
                require_cryptographic_holder_binding: None,
                claims: Some(vec![
                    ClaimQuery {
                        id: None,
                        path: ClaimPathPointer::try_new(vec![
                            ClaimPathElement::String("credentialSubject".to_string()),
                            ClaimPathElement::String("givenName".to_string())
                        ])
                        .unwrap(),
                        values: None,
                    },
                    ClaimQuery {
                        id: None,
                        path: ClaimPathPointer::try_new(vec![
                            ClaimPathElement::String("credentialSubject".to_string()),
                            ClaimPathElement::String("familyName".to_string())
                        ])
                        .unwrap(),
                        values: None,
                    },
                    ClaimQuery {
                        id: None,
                        path: ClaimPathPointer::try_new(vec![
                            ClaimPathElement::String("credentialSubject".to_string()),
                            ClaimPathElement::String("email".to_string())
                        ])
                        .unwrap(),
                        values: None,
                    },
                    ClaimQuery {
                        id: None,
                        path: ClaimPathPointer::try_new(vec![
                            ClaimPathElement::String("credentialSubject".to_string()),
                            ClaimPathElement::String("birthdate".to_string())
                        ])
                        .unwrap(),
                        values: None,
                    },
                ]),
                claim_sets: None,
            }],
            credential_sets: None,
        };
    }

    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY_OID4VP: &str = "\
        openid://?\
            client_id=decentralized_identifier%3Adid%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK_OID4VP: &str = "\
        openid://?\
            client_id=decentralized_identifier%3Adid%3Ajwk%3AeyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_KEY_SIOPV2: &str = "\
        openid://?\
            client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const FORM_URL_ENCODED_AUTHORIZATION_REQUEST_DID_JWK_SIOPV2: &str = "\
        openid://?\
            client_id=did%3Ajwk%3AeyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ&\
            request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2Fstate";
    const SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_KEY: &str = "eyJ0eXAiOiJvYXV0aC1hdXRoei1yZXErand0IiwiYWxnIjoiRVMyNTYiLCJraWQiOiJkaWQ6a2V5OnpEbmFlUndUNGc2QVpDSHp4dk5MN0RManFUYVQ4OGFtNFhSNlRVR3JLcjZEWGo2VHojekRuYWVSd1Q0ZzZBWkNIenh2Tkw3RExqcVRhVDg4YW00WFI2VFVHcktyNkRYajZUeiJ9.eyJjbGllbnRfaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInJlZGlyZWN0X3VyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL3JlZGlyZWN0Iiwic3RhdGUiOiJzdGF0ZSIsInJlc3BvbnNlX3R5cGUiOiJpZF90b2tlbiIsInNjb3BlIjoib3BlbmlkIiwicmVzcG9uc2VfbW9kZSI6ImRpcmVjdF9wb3N0Iiwibm9uY2UiOiJub25jZSIsImNsaWVudF9tZXRhZGF0YSI6eyJjbGllbnRfbmFtZSI6IlVuaUNvcmUiLCJsb2dvX3VyaSI6Imh0dHBzOi8vd3d3LmltcGllcmNlLmNvbS9leHRlcm5hbC9pbXBpZXJjZS1pY29uLnBuZyIsInN1YmplY3Rfc3ludGF4X3R5cGVzX3N1cHBvcnRlZCI6WyJkaWQ6andrIiwiZGlkOmtleSJdLCJpZF90b2tlbl9zaWduZWRfcmVzcG9uc2VfYWxnIjoiRVMyNTYiLCJpZF90b2tlbl9zaWduaW5nX2FsZ192YWx1ZXNfc3VwcG9ydGVkIjpbIkVTMjU2IiwiRWREU0EiXX19.blBC3CrvKp2lTEsT-L_LwssLyeUm00JD0__n0TiEsDR63iQY__QOKjWG5eqEnDtmHhxgE7aRh9Nf5INo20mFEw";
    const SIGNED_AUTHORIZATION_REQUEST_OBJECT_DID_JWK: &str = "eyJ0eXAiOiJvYXV0aC1hdXRoei1yZXErand0IiwiYWxnIjoiRVMyNTYiLCJraWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaWIwOVpNbVJOVmxVM1IwczFZV3d4Y1RkRlFYaDFiMWxzYjNoTlVXeDJOVnBPV2s5aGRHbFZXRkZJWnlJc0ltdDBlU0k2SWtWRElpd2llQ0k2SWtadGF6RXpaMDh5VTBkTVluVllaVXd5TkhGS1VFaERUbTVqYmtrMmJFSjFObHBSVERKRlZscDJORVVpTENKNUlqb2labm95UzNaTmFIVm1lbFZ3VFdWTU9TMUxNbkpsT1daM1FUTnRlbWN4WW5CbVltTmxTVkZUZFdsb1dTSjkjMCJ9.eyJjbGllbnRfaWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWRpSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSmlVVXRSVW5waGIzQTNRMmRGZG5GV2NUaFZiR2RNUjNOa1JpMVNMV2h1VEVaclMwWmFjVmN5Vms0d0lpd2lhM1I1SWpvaVQwdFFJaXdpZUNJNklrZHNia3M1WlZCek9EQXlXSGhCWjJ4U1QxRjZiMGQxY20wNVVYQjJNRWxHVUVWaVpFMURTVXhPWDFVaWZRIiwicmVkaXJlY3RfdXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvcmVkaXJlY3QiLCJzdGF0ZSI6InN0YXRlIiwicmVzcG9uc2VfdHlwZSI6ImlkX3Rva2VuIiwic2NvcGUiOiJvcGVuaWQiLCJyZXNwb25zZV9tb2RlIjoiZGlyZWN0X3Bvc3QiLCJub25jZSI6Im5vbmNlIiwiY2xpZW50X21ldGFkYXRhIjp7ImNsaWVudF9uYW1lIjoiVW5pQ29yZSIsImxvZ29fdXJpIjoiaHR0cHM6Ly93d3cuaW1waWVyY2UuY29tL2V4dGVybmFsL2ltcGllcmNlLWljb24ucG5nIiwic3ViamVjdF9zeW50YXhfdHlwZXNfc3VwcG9ydGVkIjpbImRpZDpqd2siLCJkaWQ6a2V5Il0sImlkX3Rva2VuX3NpZ25lZF9yZXNwb25zZV9hbGciOiJFUzI1NiIsImlkX3Rva2VuX3NpZ25pbmdfYWxnX3ZhbHVlc19zdXBwb3J0ZWQiOlsiRVMyNTYiLCJFZERTQSJdfX0.Ts-MAI9gGZcX12RtANn-3B6vvRWinElNqmfnx0YkDUIe6cLC3TudLuQLbjcLahvwyVSbz_46oIPWpll19W3ylA";
}
