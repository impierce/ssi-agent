use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError;
use crate::offer::event::OfferEvent;
use crate::services::HolderServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_credential::credential::Jwt;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants};
use oid4vci::credential_response::CredentialResponseType;
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Pending,
    Accepted,
    CredentialsReceived,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OfferCredential {
    pub holder_credential_id: String,
    pub credential: Jwt,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Offer {
    #[serde(rename = "id")]
    pub received_offer_id: String,
    pub credential_offer: Option<CredentialOfferParameters>,
    pub status: Status,
    pub credential_configurations: Option<HashMap<String, CredentialConfigurationsSupportedObject>>,
    pub token_response: Option<TokenResponse>,
    // TODO: These should not be part of this Aggregate. Instead, an Event Subscriber should be listening to the
    // `CredentialResponseReceived` event and then trigger the `CredentialCommand::AddCredential` command. We can do
    // this once we have a mechanism implemented that can both listen to events as well as trigger commands.
    pub credentials: Vec<OfferCredential>,
}

#[async_trait]
impl Aggregate for Offer {
    type Command = OfferCommand;
    type Event = OfferEvent;
    type Error = OfferError;
    type Services = Arc<HolderServices>;

    fn aggregate_type() -> String {
        "received_offer".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use OfferCommand::*;
        use OfferError::*;
        use OfferEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            ReceiveCredentialOffer {
                received_offer_id,
                credential_offer,
            } => {
                let wallet = &services.wallet;

                let credential_offer = match credential_offer {
                    CredentialOffer::CredentialOfferUri(credential_offer_uri) => services
                        .wallet
                        .get_credential_offer(credential_offer_uri)
                        .await
                        .map_err(|_| CredentialOfferByReferenceRetrievalError)?,
                    CredentialOffer::CredentialOffer(credential_offer) => *credential_offer,
                };

                // The credential offer contains a credential issuer url.
                let credential_issuer_url = credential_offer.credential_issuer.clone();

                // Get the credential issuer metadata.
                let credential_issuer_metadata = wallet
                    .get_credential_issuer_metadata(credential_issuer_url.clone())
                    .await
                    .map_err(|_| CredentialIssuerMetadataRetrievalError)?;

                let credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject> =
                    credential_issuer_metadata
                        .credential_configurations_supported
                        .iter()
                        .filter(|(id, _)| credential_offer.credential_configuration_ids.contains(id))
                        .map(|(id, credential_configuration)| (id.clone(), credential_configuration.clone()))
                        .collect();

                Ok(vec![CredentialOfferReceived {
                    received_offer_id,
                    credential_offer: Box::new(credential_offer),
                    credential_configurations,
                }])
            }
            AcceptCredentialOffer { received_offer_id } => {
                // TODO: reconsider business logic: can an offer be accepted multiple times? if not, should an error be thrown to the user?
                if self.status != Status::Pending {
                    warn!(
                        "Accepting Offer with ID: {received_offer_id} while its current status is: {:?}",
                        self.status
                    );
                }

                let wallet = &services.wallet;

                let credential_offer = self.credential_offer.as_ref().ok_or(MissingCredentialOfferError)?;

                let credential_issuer_url = credential_offer.credential_issuer.clone();

                // Get the authorization server metadata.
                let authorization_server_metadata = wallet
                    .get_authorization_server_metadata(credential_issuer_url.clone())
                    .await
                    .map_err(|_| AuthorizationServerMetadataRetrievalError)?;

                // Create a token request with grant_type `pre_authorized_code`.
                let token_request = match credential_offer.grants.clone() {
                    Some(Grants {
                        pre_authorized_code: Some(pre_authorized_code),
                        // TODO: support `authorization_code` grant type.
                        ..
                    }) => TokenRequest::PreAuthorizedCode {
                        pre_authorized_code: pre_authorized_code.pre_authorized_code,
                        tx_code: None,
                    },
                    _ => return Err(MissingPreAuthorizedCodeError),
                };

                info!("token_request: {:?}", token_request);

                // Get an access token.
                let token_response = wallet
                    .get_access_token(
                        authorization_server_metadata
                            .token_endpoint
                            .ok_or(MissingTokenEndpointError)?,
                        token_request,
                    )
                    .await
                    .map_err(|_| TokenResponseError)?;

                info!("token_response: {:?}", token_response);

                Ok(vec![
                    CredentialOfferAccepted {
                        received_offer_id: received_offer_id.clone(),
                        status: Status::Accepted,
                    },
                    TokenResponseReceived {
                        received_offer_id,
                        token_response,
                    },
                ])
            }
            SendCredentialRequest { received_offer_id } => {
                if self.status != Status::Accepted {
                    return Err(CredentialOfferStatusNotAcceptedError);
                }

                let wallet = &services.wallet;

                let credential_offer = self.credential_offer.as_ref().ok_or(MissingCredentialOfferError)?;

                let credential_issuer_url = credential_offer.credential_issuer.clone();

                // Get an access token.
                let token_response = self.token_response.as_ref().ok_or(MissingTokenResponseError)?.clone();

                let credential_configuration_ids = credential_offer.credential_configuration_ids.clone();

                // Get the credential issuer metadata.
                let credential_issuer_metadata = wallet
                    .get_credential_issuer_metadata(credential_issuer_url.clone())
                    .await
                    .map_err(|_| CredentialIssuerMetadataRetrievalError)?;

                // Get the authorization server metadata.
                let authorization_server_metadata = wallet
                    .get_authorization_server_metadata(credential_issuer_url.clone())
                    .await
                    .map_err(|_| AuthorizationServerMetadataRetrievalError)?;

                let credential_configurations = self
                    .credential_configurations
                    .as_ref()
                    .ok_or(MissingCredentialConfigurationsError)?;

                let credentials: Vec<OfferCredential> = match credential_configuration_ids.len() {
                    0 => vec![],
                    1 => {
                        // Get a nonce if the credential issuer metadata contains a nonce endpoint.
                        let nonce = if let Some(nonce_endpoint) = &credential_issuer_metadata.nonce_endpoint {
                            wallet.get_nonce(nonce_endpoint.clone()).await.ok()
                        } else {
                            None
                        };

                        info!("nonce: {nonce:?}");

                        // Determine if anonymous access is supported.
                        // See: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-15.html#section-8.2.1.1-2.2.2.2
                        let with_anonymous_access = authorization_server_metadata
                            .pre_authorized_grant_anonymous_access_supported
                            .unwrap_or(false);

                        let credential_configuration_id = &credential_configuration_ids[0];

                        let credential_configuration = credential_configurations
                            .get(credential_configuration_id)
                            .ok_or(MissingCredentialConfigurationError)?;

                        // Get the credential.
                        let credential_response = wallet
                            .get_credential(
                                credential_issuer_metadata,
                                &token_response,
                                nonce,
                                credential_configuration_id.clone(),
                                credential_configuration,
                                with_anonymous_access,
                            )
                            .await
                            .map_err(|_| CredentialResponseError)?;

                        let credential = match credential_response.credential {
                            CredentialResponseType::Immediate { credentials, .. } => {
                                Jwt::from(credentials[0].credential.clone())
                            }
                            CredentialResponseType::Deferred { .. } => {
                                return Err(UnsupportedDeferredCredentialResponseError)
                            }
                        };

                        #[cfg(not(feature = "test_utils"))]
                        let holder_credential_id = uuid::Uuid::new_v4().to_string();
                        #[cfg(feature = "test_utils")]
                        let holder_credential_id = test_utils::holder_credential_id();

                        vec![OfferCredential {
                            holder_credential_id,
                            credential,
                        }]
                    }
                    _batch => {
                        return Err(BatchCredentialRequestError);
                    }
                };

                info!("credentials: {credentials:?}");

                Ok(vec![CredentialResponseReceived {
                    received_offer_id,
                    status: Status::CredentialsReceived,
                    credentials,
                }])
            }
            RejectCredentialOffer { received_offer_id } => {
                // TODO: should we 'do nothing' or log a `warn!` message instead of returning an error?
                if self.status != Status::Pending {
                    return Err(CredentialOfferStatusNotPendingError);
                }

                Ok(vec![CredentialOfferRejected {
                    received_offer_id,
                    status: Status::Rejected,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            CredentialOfferReceived {
                credential_offer,
                credential_configurations,
                ..
            } => {
                self.credential_offer.replace(*credential_offer);
                self.credential_configurations.replace(credential_configurations);
            }
            CredentialOfferAccepted { status, .. } => {
                self.status = status;
            }
            TokenResponseReceived { token_response, .. } => {
                self.token_response.replace(token_response);
            }
            CredentialResponseReceived {
                status, credentials, ..
            } => {
                self.status = status;
                self.credentials = credentials;
            }
            CredentialOfferRejected { status, .. } => {
                self.status = status;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::test_utils::*;
    use super::*;
    use agent_api_rest::API_VERSION;
    use agent_api_rest::{authorization, issuance};
    use agent_issuance::server_config::aggregate::test_utils::credential_configurations_supported;
    use agent_secret_manager::service::Service;
    use agent_shared::config::config;
    use agent_shared::config::config_mut;
    use agent_shared::generate_random_string;
    use agent_store::in_memory::InMemory;
    use agent_store::{authorization_state, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use cqrs_es::test::TestFramework;
    use oid4vci::credential_offer::CredentialOffer;
    use rstest::{fixture, rstest};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tower::Service as _;

    type OfferTestFramework = TestFramework<Offer>;

    async fn bootstrap_issuer_server() -> CredentialOffer {
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let issuer_url = format!("http://{}", listener.local_addr().unwrap());

        config_mut().application_url = issuer_url.parse().unwrap();

        let application_url = config().application_url.clone();

        config_mut().public_url = application_url.clone();
        config_mut().token_endpoint = application_url.join("auth/token").unwrap();
        config_mut().credential_endpoint = application_url.join("openid4vci/credential").unwrap();
        config_mut().credential_offer_uri = application_url.join("openid4vci/credential-offer/").unwrap();

        let issuance_state = issuance_state(&InMemory, Service::default(), Default::default()).await;
        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let mut credential_isser = issuance::router(issuance_state.clone());

        let authorization_state = authorization_state(&InMemory, Service::default(), Default::default()).await;
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();
        let authorization_server = authorization::router((authorization_state, issuance_state));

        let received_offer_id = generate_random_string();

        let _ = credential_isser
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{issuer_url}{API_VERSION}/credentials"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": received_offer_id,
                            "credential": {
                                "credentialSubject": {
                                    "first_name": "Ferris",
                                    "last_name": "Rustacean",
                                    "degree": {
                                        "type": "MasterDegree",
                                        "name": "Master of Oceanography"
                                    }
                            }},
                            "credentialConfigurationId": "001",
                            "expiresAt": "never"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await;

        let response = credential_isser
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{issuer_url}{API_VERSION}/offers"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": received_offer_id,
                            "credentialConfigurationIds": ["001"]
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let credential_offer: CredentialOffer = String::from_utf8(body.to_vec()).unwrap().parse().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, credential_isser.merge(authorization_server))
                .await
                .unwrap();
        });

        credential_offer
    }

    #[fixture]
    async fn credential_offer_parameters() -> Box<CredentialOfferParameters> {
        let credential_offer = bootstrap_issuer_server().await;

        match credential_offer {
            CredentialOffer::CredentialOffer(credential_offer) => credential_offer,
            _ => unreachable!(),
        }
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_receive_credential_offer(
        received_offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given_no_previous_events()
            .when_async(OfferCommand::ReceiveCredentialOffer {
                received_offer_id: received_offer_id.clone(),
                credential_offer: CredentialOffer::CredentialOffer(credential_offer_parameters.clone()),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialOfferReceived {
                received_offer_id,
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }]);
    }

    // TODO: Update this test after Application logic is migrated to the Application layer. This test is currently
    // expected to fail due to the `access_token` in the `TokenResponse` being a dynamic JWT now which means that we
    // cannot make use of a static Access Token token value for testing anymore. This test is still useful to ensure
    // that the `OfferAggregate` is able to handle the `AcceptCredentialOffer` command and produce the expected events
    // and expected 'panic' message.
    #[should_panic(
        expected = "access_token: \"eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9"
    )]
    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_accept_credential_offer(
        received_offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferReceived {
                received_offer_id: received_offer_id.clone(),
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }])
            .when_async(OfferCommand::AcceptCredentialOffer {
                received_offer_id: received_offer_id.clone(),
            })
            .await
            .then_expect_events(vec![
                OfferEvent::CredentialOfferAccepted {
                    received_offer_id: received_offer_id.clone(),
                    status: Status::Accepted,
                },
                OfferEvent::TokenResponseReceived {
                    received_offer_id,
                    token_response: TokenResponse {
                        // Placeholder Access Token which is expected to fail validation in the test.
                        access_token: "placeholder".to_string(),
                        token_type: "Bearer".to_string(),
                        expires_in: None,
                        scope: None,
                        refresh_token: None,
                    },
                },
            ]);
    }

    // TODO: Similar to the test above, this test is expected to fail due to the dynamic JWT in the `TokenResponse`.
    // Sending HTTP requests as well as the signing and validation of JWTs is Application layer logic so these things
    // should not be implemented/tested here. For now we can expect this test to panic with the message "An error
    // occurred while requesting the credentials" which is the expected behavior when the `OfferAggregate` tries to
    // request credentials with the invalid placeholder Access Token.
    #[should_panic(expected = "An error occurred while requesting the credentials")]
    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_send_credential_request(
        received_offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
        signed_credentials: Vec<OfferCredential>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferReceived {
                    received_offer_id: received_offer_id.clone(),
                    credential_offer: credential_offer_parameters,
                    credential_configurations: credential_configurations_supported,
                },
                OfferEvent::CredentialOfferAccepted {
                    received_offer_id: received_offer_id.clone(),
                    status: Status::Accepted,
                },
                OfferEvent::TokenResponseReceived {
                    received_offer_id: received_offer_id.clone(),
                    token_response: TokenResponse {
                        // Placeholder Access Token which is expected to fail validation in the test.
                        access_token: "placeholder".to_string(),
                        token_type: "Bearer".to_string(),
                        expires_in: None,
                        scope: None,
                        refresh_token: None,
                    },
                },
            ])
            .when_async(OfferCommand::SendCredentialRequest {
                received_offer_id: received_offer_id.clone(),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialResponseReceived {
                received_offer_id: received_offer_id.clone(),
                status: Status::CredentialsReceived,
                credentials: signed_credentials,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_reject_credential_offer(
        received_offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferReceived {
                received_offer_id: received_offer_id.clone(),
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }])
            .when_async(OfferCommand::RejectCredentialOffer {
                received_offer_id: received_offer_id.clone(),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialOfferRejected {
                received_offer_id: received_offer_id.clone(),
                status: Status::Rejected,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use agent_shared::generate_random_string;
    use identity_credential::credential::Jwt;
    use rstest::*;

    #[fixture]
    pub fn received_offer_id() -> String {
        generate_random_string()
    }

    #[fixture]
    pub fn holder_credential_id() -> String {
        "holder_credential_id".to_string()
    }

    #[fixture]
    pub fn signed_credentials(holder_credential_id: String) -> Vec<OfferCredential> {
        vec![OfferCredential { holder_credential_id, credential: Jwt::from("eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ekRuYWVSd1Q0ZzZBWkNIenh2Tkw3RExqcVRhVDg4YW00WFI2VFVHcktyNkRYajZUeiIsIm5iZiI6MTI2MjMwNDAwMCwiaWF0IjoxMjYyMzA0MDAwLCJ2YyI6eyJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSJdLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6RG5hZVJ3VDRnNkFaQ0h6eHZOTDdETGpxVGFUODhhbTRYUjZUVUdyS3I2RFhqNlR6IiwiZGVncmVlIjp7InR5cGUiOiJNYXN0ZXJEZWdyZWUiLCJuYW1lIjoiTWFzdGVyIG9mIE9jZWFub2dyYXBoeSJ9LCJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIn0sImlzc3VlciI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJjcmVkZW50aWFsU3RhdHVzIjp7ImlkIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwidHlwZSI6InN0YXR1c2xpc3Qrand0IiwiaWR4IjoxMjMsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCJ9fSwic3RhdHVzIjp7InN0YXR1c19saXN0Ijp7ImlkeCI6MTIzLCJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAifX19.6oVjfN06dzQmd2oCVBm9lgOkhL0mLIHn-I8vUB0OX3n7MbjhIWjjfA_6SSvcofGGLj-BFt1fMCDUy1VriCviAA".to_string())}]
    }
}
