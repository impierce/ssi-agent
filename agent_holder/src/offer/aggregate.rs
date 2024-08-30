use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError;
use crate::offer::event::OfferEvent;
use crate::services::HolderServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants};
use oid4vci::credential_response::CredentialResponseType;
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Pending,
    Accepted,
    Received,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Offer {
    pub credential_offer: Option<CredentialOfferParameters>,
    pub status: Status,
    pub credential_configurations: Option<HashMap<String, CredentialConfigurationsSupportedObject>>,
    pub token_response: Option<TokenResponse>,
    // TODO: These should not be part of this Aggregate. Instead, an Event Subscriber should be listening to the
    // `CredentialResponseReceived` event and then trigger the `CredentialCommand::AddCredential` command. We can do
    // this once we have a mechanism implemented that can both listen to events as well as trigger commands.
    pub credentials: Vec<serde_json::Value>,
}

#[async_trait]
impl Aggregate for Offer {
    type Command = OfferCommand;
    type Event = OfferEvent;
    type Error = OfferError;
    type Services = Arc<HolderServices>;

    fn aggregate_type() -> String {
        "offer".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use OfferCommand::*;
        use OfferEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            ReceiveCredentialOffer {
                offer_id,
                credential_offer,
            } => {
                let wallet = &services.wallet;

                let credential_offer = match credential_offer {
                    CredentialOffer::CredentialOfferUri(credential_offer_uri) => services
                        .wallet
                        .get_credential_offer(credential_offer_uri)
                        .await
                        .unwrap(),
                    CredentialOffer::CredentialOffer(credential_offer) => *credential_offer,
                };

                // The credential offer contains a credential issuer url.
                let credential_issuer_url = credential_offer.credential_issuer.clone();

                // Get the credential issuer metadata.
                let credential_issuer_metadata = wallet
                    .get_credential_issuer_metadata(credential_issuer_url.clone())
                    .await
                    .unwrap();

                let credential_configurations: HashMap<String, CredentialConfigurationsSupportedObject> =
                    credential_issuer_metadata
                        .credential_configurations_supported
                        .iter()
                        .filter(|(id, _)| credential_offer.credential_configuration_ids.contains(id))
                        .map(|(id, credential_configuration)| (id.clone(), credential_configuration.clone()))
                        .collect();

                Ok(vec![CredentialOfferReceived {
                    offer_id,
                    credential_offer: Box::new(credential_offer),
                    credential_configurations,
                }])
            }
            AcceptCredentialOffer { offer_id } => {
                let wallet = &services.wallet;

                let credential_issuer_url = self.credential_offer.as_ref().unwrap().credential_issuer.clone();

                // Get the authorization server metadata.
                let authorization_server_metadata = wallet
                    .get_authorization_server_metadata(credential_issuer_url.clone())
                    .await
                    .unwrap();

                // Create a token request with grant_type `pre_authorized_code`.
                let token_request = match self.credential_offer.as_ref().unwrap().grants.clone() {
                    Some(Grants {
                        pre_authorized_code, ..
                    }) => TokenRequest::PreAuthorizedCode {
                        pre_authorized_code: pre_authorized_code.unwrap().pre_authorized_code,
                        tx_code: None,
                    },
                    None => unreachable!(),
                };

                info!("token_request: {:?}", token_request);

                // Get an access token.
                let token_response = wallet
                    .get_access_token(authorization_server_metadata.token_endpoint.unwrap(), token_request)
                    .await
                    .unwrap();

                info!("token_response: {:?}", token_response);

                Ok(vec![
                    CredentialOfferAccepted {
                        offer_id: offer_id.clone(),
                        status: Status::Accepted,
                    },
                    TokenResponseReceived {
                        offer_id,
                        token_response,
                    },
                ])
            }
            SendCredentialRequest { offer_id } => {
                let wallet = &services.wallet;

                let credential_issuer_url = self.credential_offer.as_ref().unwrap().credential_issuer.clone();

                // Get an access token.
                let token_response = self.token_response.as_ref().unwrap().clone();

                let credential_configuration_ids = self
                    .credential_offer
                    .as_ref()
                    .unwrap()
                    .credential_configuration_ids
                    .clone();

                // Get the credential issuer metadata.
                let credential_issuer_metadata = wallet
                    .get_credential_issuer_metadata(credential_issuer_url.clone())
                    .await
                    .unwrap();

                let credentials: Vec<serde_json::Value> = match credential_configuration_ids.len() {
                    0 => vec![],
                    1 => {
                        let credential_configuration_id = credential_configuration_ids[0].clone();

                        let credential_configuration = self
                            .credential_configurations
                            .as_ref()
                            .unwrap()
                            .get(&credential_configuration_id)
                            .unwrap();

                        // Get the credential.
                        let credential_response = wallet
                            .get_credential(credential_issuer_metadata, &token_response, credential_configuration)
                            .await
                            .unwrap();

                        let credential = match credential_response.credential {
                            CredentialResponseType::Immediate { credential, .. } => credential,
                            _ => panic!("Credential was not a jwt_vc_json."),
                        };

                        vec![credential]
                    }
                    _batch => {
                        todo!()
                    }
                };

                info!("credentials: {:?}", credentials);

                Ok(vec![CredentialResponseReceived {
                    offer_id,
                    status: Status::Received,
                    credentials,
                }])
            }
            RejectCredentialOffer { offer_id } => Ok(vec![CredentialOfferRejected {
                offer_id,
                status: Status::Rejected,
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        info!("Applying event: {:?}", event);

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
    use agent_api_rest::issuance;
    use agent_api_rest::API_VERSION;
    use agent_issuance::offer::aggregate::test_utils::token_response;
    use agent_issuance::server_config::aggregate::test_utils::credential_configurations_supported;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_shared::generate_random_string;
    use agent_store::in_memory;
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

        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(issuer_url.parse().unwrap())).await;

        let offer_id = generate_random_string();

        let mut app = issuance::router(issuance_state);

        let _ = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(&format!("{issuer_url}{API_VERSION}/credentials"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": offer_id,
                            "credential": {
                                "credentialSubject": {
                                    "first_name": "Ferris",
                                    "last_name": "Rustacean",
                                    "degree": {
                                        "type": "MasterDegree",
                                        "name": "Master of Oceanography"
                                    }
                            }},
                            "credentialConfigurationId": "badge"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await;

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(&format!("{issuer_url}{API_VERSION}/offers"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": offer_id
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
            axum::serve(listener, app).await.unwrap();
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
        offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given_no_previous_events()
            .when_async(OfferCommand::ReceiveCredentialOffer {
                offer_id: offer_id.clone(),
                credential_offer: CredentialOffer::CredentialOffer(credential_offer_parameters.clone()),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialOfferReceived {
                offer_id,
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_accept_credential_offer(
        offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        #[future(awt)] token_response: TokenResponse,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferReceived {
                offer_id: offer_id.clone(),
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }])
            .when_async(OfferCommand::AcceptCredentialOffer {
                offer_id: offer_id.clone(),
            })
            .await
            .then_expect_events(vec![
                OfferEvent::CredentialOfferAccepted {
                    offer_id: offer_id.clone(),
                    status: Status::Accepted,
                },
                OfferEvent::TokenResponseReceived {
                    offer_id,
                    token_response,
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_send_credential_request(
        offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        #[future(awt)] token_response: TokenResponse,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferReceived {
                    offer_id: offer_id.clone(),
                    credential_offer: credential_offer_parameters,
                    credential_configurations: credential_configurations_supported,
                },
                OfferEvent::CredentialOfferAccepted {
                    offer_id: offer_id.clone(),
                    status: Status::Accepted,
                },
                OfferEvent::TokenResponseReceived {
                    offer_id: offer_id.clone(),
                    token_response
                },
            ])
            .when_async(OfferCommand::SendCredentialRequest {
                offer_id: offer_id.clone(),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialResponseReceived {
                offer_id: offer_id.clone(),
                status: Status::Received,
                credentials: vec![json!("eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjoiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJkZWdyZWUiOnsidHlwZSI6Ik1hc3RlckRlZ3JlZSIsIm5hbWUiOiJNYXN0ZXIgb2YgT2NlYW5vZ3JhcGh5In0sImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4ifSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiJ9fQ.jQEpI7DhjOcmyhPEpfGARwcRyzor_fUvynb43-eqD9175FBoshENX0S-8qlloQ7vbT5gat8TjvcDlGDN720ZBw")],
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_reject_credential_offer(
        offer_id: String,
        #[future(awt)] credential_offer_parameters: Box<CredentialOfferParameters>,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferReceived {
                offer_id: offer_id.clone(),
                credential_offer: credential_offer_parameters,
                credential_configurations: credential_configurations_supported,
            }])
            .when_async(OfferCommand::RejectCredentialOffer {
                offer_id: offer_id.clone(),
            })
            .await
            .then_expect_events(vec![OfferEvent::CredentialOfferRejected {
                offer_id: offer_id.clone(),
                status: Status::Rejected,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_shared::generate_random_string;
    use rstest::*;

    #[fixture]
    pub fn offer_id() -> String {
        generate_random_string()
    }
}
