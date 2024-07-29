use super::{command::ConnectionCommand, error::ConnectionError, event::ConnectionEvent};
use crate::{generic_oid4vc::GenericAuthorizationResponse, services::VerificationServices};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vp::Oid4vpParams;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, vec};
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Connection {
    // TODO: Does user data need to be stored in UniCore at all?
    id_token: Option<String>,
    vp_token: Option<String>,
    state: Option<String>,
}

#[async_trait]
impl Aggregate for Connection {
    type Command = ConnectionCommand;
    type Event = ConnectionEvent;
    type Error = ConnectionError;
    type Services = Arc<VerificationServices>;

    fn aggregate_type() -> String {
        "connection".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ConnectionCommand::*;
        use ConnectionError::*;
        use ConnectionEvent::*;

        info!("Handling command: {:?}", command);

        match command {
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
                        let _ = relying_party
                            .validate_response(&oid4vp_authorization_response)
                            .await
                            .map_err(InvalidOID4VPAuthorizationResponse)?;

                        let vp_token = match oid4vp_authorization_response.extension.oid4vp_parameters {
                            Oid4vpParams::Params { vp_token, .. } => vp_token,
                            Oid4vpParams::Jwt { .. } => return Err(UnsupportedJwtParameterError),
                        };

                        Ok(vec![OID4VPAuthorizationResponseVerified {
                            vp_token,
                            state: oid4vp_authorization_response.state,
                        }])
                    }
                }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ConnectionEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            SIOPv2AuthorizationResponseVerified { id_token, state } => {
                self.id_token.replace(id_token);
                self.state = state;
            }
            OID4VPAuthorizationResponseVerified { vp_token, state } => {
                self.vp_token.replace(vp_token);
                self.state = state;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;
    use agent_shared::config::SupportedDidMethod;
    use cqrs_es::test::TestFramework;
    use identity_credential::credential::Jwt;
    use identity_credential::presentation::Presentation;

    use agent_shared::config::set_config;
    use jsonwebtoken::Algorithm;
    use oid4vc_manager::managers::presentation::create_presentation_submission;
    use oid4vc_manager::ProviderManager;
    use oid4vci::VerifiableCredentialJwt;
    use oid4vp::oid4vp::AuthorizationResponseInput;
    use rstest::rstest;

    use crate::authorization_request::aggregate::tests::{
        authorization_request, verifier_did, PRESENTATION_DEFINITION,
    };
    use crate::generic_oid4vc::GenericAuthorizationRequest;
    use crate::services::test_utils::test_verification_services;

    use super::*;

    type ConnectionTestFramework = TestFramework<Connection>;

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

        let verification_services = test_verification_services();
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

        ConnectionTestFramework::with(verification_services)
            .given_no_previous_events()
            .when(ConnectionCommand::VerifyAuthorizationResponse {
                authorization_request,
                authorization_response,
            })
            .then_expect_events(vec![match response_type {
                "id_token" => ConnectionEvent::SIOPv2AuthorizationResponseVerified {
                    id_token: token,
                    state: Some("state".to_string()),
                },
                "vp_token" => ConnectionEvent::OID4VPAuthorizationResponseVerified {
                    vp_token: token,
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
                    secret_manager: secret_manager().await,
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
                                verifiable_presentation,
                                presentation_submission,
                            },
                        )
                        .await
                        .unwrap(),
                )
            }
        }
    }
}
