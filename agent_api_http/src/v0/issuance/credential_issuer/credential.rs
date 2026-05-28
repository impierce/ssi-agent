use std::time::{Duration, Instant};

use crate::{
    handlers::{command_handler, query_handler},
    v0::issuance::error::internal_server_error,
    v0::issuance::error::PublicError,
};
use agent_issuance::{
    application::{
        access_token_validation_service::{AccessTokenValidationError, AccessTokenValidationService},
        nonce_validation_service::NonceValidationService,
    },
    credential::{command::CredentialCommand, views::CredentialView},
    offer::{command::OfferCommand, views::OfferView},
    server_config::views::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
    status_list::command::StatusListCommand,
};
use agent_shared::config::{config, BITS_PER_STATUS, STATUS_LIST_BYTES_AMOUNT};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use oauth_tsl::status_list::StatusType;
use oid4vci::credential_request::CredentialRequest;
use oid4vci::errors::CredentialErrorResponse;
use std::sync::Arc;
use tokio::time::sleep;
use tracing::error;

#[cfg(feature = "test_utils")]
use agent_shared::config::TEST_STATUS_LIST_ID;

const POLLING_INTERVAL_MS: u64 = 100;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<IssuanceState>>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> Result<Response, PublicError> {
    let claims = AccessTokenValidationService::validate(&state, &access_token).await?;

    // The Access Token must contain the `issuer_state` claim, which is used to identify the `offer_id`.
    let offer_id = claims
        .issuer_state
        .ok_or_else(|| PublicError::from(AccessTokenValidationError::InvalidToken))?;

    NonceValidationService::validate(&state, &credential_request)
        .await
        .map_err(|_| PublicError::from(CredentialErrorResponse::InvalidNonce))?;

    // Get the `credential_issuer_metadata` and `authorization_server_metadata` from the `ServerConfigView`.
    let (credential_issuer_metadata, authorization_server_metadata) =
        match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
            Some(ServerConfigView {
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_configurations: _,
                cryptographic_binding_methods_supported: _,
                signing_algorithms_supported: _,
            }) => (
                Box::new(credential_issuer_metadata),
                Box::new(authorization_server_metadata),
            ),
            _ => return Err(internal_server_error()),
        };

    let proofs = credential_request.proofs.clone();

    let command = OfferCommand::VerifyCredentialRequest {
        offer_id: offer_id.clone(),
        credential_issuer_metadata,
        authorization_server_metadata,
        credential_request,
    };

    // Use the `offer_id` to verify the `proof` inside the `CredentialRequest`.
    command_handler(&offer_id, &state.command.offer, command).await?;

    let timeout = config().external_server_response_timeout_ms;
    let start_time = Instant::now();

    // TODO: replace this polling solution with a call to the `TxChannelRegistry` as described here: https://github.com/impierce/ssi-agent/issues/75
    // Use the `offer_id` to get the `credential_ids` and `subject_id` from the `OfferView`.
    let (credential_ids, subject_id) = loop {
        match query_handler(&offer_id, &state.query.offer).await? {
            // When the Offer does not include the credential id's yet, wait for the external server to provide them.
            Some(OfferView { credential_ids, .. }) if credential_ids.is_empty() => {
                if start_time.elapsed().as_millis() <= timeout as u128 {
                    sleep(Duration::from_millis(POLLING_INTERVAL_MS)).await;
                } else {
                    error!("timeout failure");
                    return Err(internal_server_error());
                }
            }
            Some(OfferView {
                credential_ids,
                subject_id,
                ..
            }) => {
                break (credential_ids, subject_id);
            }
            _ => {
                return Err(internal_server_error());
            }
        }
    };

    let all_status_lists = query_handler("all_status_lists", &state.query.all_status_lists)
        .await?
        .map(|all_status_lists_view| all_status_lists_view.status_lists.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    // Find a status list which is not full
    // TODO: this contains quite some domain logic and should actually be moved there.
    let max_amount_indices = STATUS_LIST_BYTES_AMOUNT * 8 / BITS_PER_STATUS as usize;
    let available_status_list = all_status_lists
        .into_iter()
        .find(|status_list| status_list.used_indices.len() + credential_ids.len() <= (max_amount_indices * 7) / 10); // The 7/10 factor ensures status lists are at most 70% full, the remainder will always be preserverd for random values protecting Issuer and Holder privacy.

    let status_list_id = match available_status_list {
        Some(status_list) => status_list.id.clone(),
        None => {
            #[cfg(not(feature = "test_utils"))]
            let id = uuid::Uuid::new_v4().to_string();

            #[cfg(feature = "test_utils")]
            let id = TEST_STATUS_LIST_ID.to_string();

            let command = StatusListCommand::CreateStatusList { id: id.clone() };
            command_handler(&id, &state.command.status_list, command).await?;

            id
        }
    };

    // Use the `credential_ids` and `subject_id` to sign all the credentials.
    let mut signed_credentials = vec![];
    for credential_id in credential_ids {
        let command = StatusListCommand::AddIndex {
            status: StatusType::VALID,
        };

        command_handler(&status_list_id, &state.command.status_list, command).await?;

        let status_list = query_handler(&status_list_id, &state.query.status_list)
            .await?
            .ok_or(PublicError::InternalServerError)?;

        let command = CredentialCommand::SignCredential {
            credential_id: credential_id.clone(),
            subject_id: subject_id.clone(),
            overwrite: false,
            proofs: proofs.clone(),
            status_list_id: status_list_id.clone(),
            index: status_list
                .used_indices
                .last()
                .cloned()
                .ok_or(PublicError::InternalServerError)?, // TODO: even though the AddIndex command is executed right before this, retrieving the index this way is not the prettiest since something of a "data race" could occur where another index has already been added between the AddIndex command and this command. Then two credentials would be assigned the same index, as they both retrieve the same last index.
        };

        command_handler(&credential_id, &state.command.credential, command).await?;

        let signed_credential = match query_handler(&credential_id, &state.query.credential).await? {
            Some(CredentialView {
                signed: Some(signed_credential),
                notification_id,
                ..
            }) => (signed_credential, notification_id),
            _ => return Err(internal_server_error()),
        };

        signed_credentials.push(signed_credential);
    }

    let command = OfferCommand::CreateCredentialResponse {
        offer_id: offer_id.clone(),
        signed_credentials,
    };

    // Use the `offer_id` to create a `CredentialResponse` from the `CredentialRequest` and `credentials`.
    command_handler(&offer_id, &state.command.offer, command).await?;

    // Use the `offer_id` to get the `credential_response` from the `OfferView`.
    query_handler(&offer_id, &state.query.offer)
        .await?
        .and_then(|offer_view| offer_view.credential_response)
        .map(|credential_response| (StatusCode::OK, Json(credential_response)).into_response())
        .ok_or_else(internal_server_error)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::v0::authorization;
    use crate::v0::authorization::authorization_server::token::tests::token;
    use crate::v0::issuance::credentials::tests::credentials;
    use crate::v0::issuance::router;
    use crate::API_VERSION;
    use crate::{
        tests::OFFER_ID,
        v0::issuance::{credentials::CredentialsEndpointRequest, offers::tests::offers},
    };

    use agent_authorization::services::AuthorizationServices;
    use agent_event_publisher_http::EventPublisherHttp;
    use agent_issuance::credential::aggregate::CredentialExpiry;
    use agent_issuance::offer::event::OfferEvent;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::IssuanceState;
    use agent_secret_manager::service::Service;
    use agent_shared::config::{set_config, Events};
    use agent_store::authorization_state;
    use agent_store::{in_memory::InMemory, issuance_state, EventPublisher};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use rstest::rstest;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsInZjIjp7ImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwibmFtZSI6IlZlcmlmaWFibGUgQ3JlZGVudGlhbCIsImlzc3VlciI6eyJuYW1lIjoiVW5pQ29yZSIsImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifSwiQGNvbnRleHQiOlsiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiXSwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJ2YWxpZEZyb20iOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsImNyZWRlbnRpYWxTdGF0dXMiOnsidHlwZSI6InN0YXR1c2xpc3Qrand0IiwiaWQiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJpZHgiOjEyM319LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fX0.Osw5UpYXtsHoomEeeJ9qz6St5b4SmpBGZL8zFmvIsBfWW114BDuQQyVwUpfvZBRuG_oxlyd-uhSRJvmJbmM6DQ";
    const ANONYMOUS_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsIm5iZiI6MTI2MjMwNDAwMCwiaWF0IjoxMjYyMzA0MDAwLCJ2YyI6eyJjcmVkZW50aWFsU3ViamVjdCI6eyJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIn0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwibmFtZSI6IlZlcmlmaWFibGUgQ3JlZGVudGlhbCIsImlzc3VlciI6eyJuYW1lIjoiVW5pQ29yZSIsImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifSwiQGNvbnRleHQiOlsiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiXSwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJ2YWxpZEZyb20iOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsImNyZWRlbnRpYWxTdGF0dXMiOnsidHlwZSI6InN0YXR1c2xpc3Qrand0IiwiaWQiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJpZHgiOjEyM319LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fX0.WpEgTuLz4ql25ohV4bcUU_qkopcS9PK4x3AieDJR0e_9zVNmjYrts_NFH_6GvoyfgFjvO4_IrOzIxmqRfdn0DA";
    const DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS: u64 = 1000;
    pub const TEST_NONCE: &str = "7e03ad3f76cb3338c3a5642fe7634476aa3ad93fa1d584011ba2150d9da47133";

    trait CredentialEventTrigger {
        async fn prepare_credential_event_trigger(
            &self,
            app: Arc<Mutex<Option<Router>>>,
            is_self_signed: bool,
            delay: u64,
        );
    }

    // Adds a method to `MockServer` which can be used to mount a mock endpoint that will be triggered when a
    // `CredentialRequestVerified` event is dispatched from the `UniCore` server. The `MockServer` used in this test
    // module must be seen as a representation of an outside backend server.
    impl CredentialEventTrigger for MockServer {
        async fn prepare_credential_event_trigger(
            &self,
            app: Arc<Mutex<Option<Router>>>,
            is_self_signed: bool,
            delay: u64,
        ) {
            Mock::given(method("POST"))
                .and(path("/ssi-events-subscriber"))
                .and(
                    move |request: &wiremock::Request| match request.body_json::<OfferEvent>().unwrap() {
                        // Validate that the event is a `CredentialRequestVerified` event.
                        OfferEvent::CredentialRequestVerified { offer_id, subject_id } => {
                            let app_clone = app.clone();

                            futures::executor::block_on(async {
                                let app_clone = app_clone.lock().await.take().unwrap();

                                // This assertion is a representation of the 'outside' backend server retrieving the
                                // data that corresponds to the `offer_id`.
                                assert_eq!(offer_id, OFFER_ID);

                                // The 'backend' server can either opt for an already signed credential...
                                let credentials_endpoint_request = if is_self_signed {
                                    CredentialsEndpointRequest {
                                        offer_id: offer_id.clone(),
                                        credential: json!(CREDENTIAL_JWT),
                                        is_signed: true,
                                        credential_configuration_id: "001".to_string(),
                                        expires_at: CredentialExpiry::Never,
                                    }
                                } else {
                                    // ...or else, submitting the data that will be signed inside `UniCore`.
                                    CredentialsEndpointRequest {
                                        offer_id: offer_id.clone(),
                                        credential: json!({
                                            "credentialSubject": {
                                                "first_name": "Ferris",
                                                "last_name": "Rustacean",
                                                "id": subject_id
                                            }
                                        }),
                                        is_signed: false,
                                        credential_configuration_id: "001".to_string(),
                                        expires_at: CredentialExpiry::Never,
                                    }
                                };

                                std::thread::sleep(Duration::from_millis(delay));

                                // Sends the `CredentialsRequest` to the `credentials` endpoint.
                                app_clone
                                    .oneshot(
                                        Request::builder()
                                            .method(http::Method::POST)
                                            .uri(format!("{API_VERSION}/credentials"))
                                            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                                            .body(Body::from(
                                                serde_json::to_vec(&credentials_endpoint_request).unwrap(),
                                            ))
                                            .unwrap(),
                                    )
                                    .await
                            })
                            .unwrap();

                            true
                        }
                        _ => false,
                    },
                )
                .respond_with(ResponseTemplate::new(200))
                .mount(self)
                .await;
        }
    }

    pub async fn credential(
        issuance_app: &mut Router,
        issuance_state: &Arc<IssuanceState>,
        access_token: String,
        external_server: Option<MockServer>,
    ) -> (String, String) {
        let command = agent_issuance::nonce::command::NonceCommand::GenerateNonce {
            c_nonce: TEST_NONCE.to_string(),
        };
        agent_shared::handlers::command_handler(TEST_NONCE, &issuance_state.command.nonce, command)
            .await
            .unwrap();

        let response = issuance_app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": "001",
                            "proofs": { "jwt": ["eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lk\
                                        IjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4\
                                        a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lG\
                                        OGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFa\
                                        djdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8v\
                                        ZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAs\
                                        Im5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkz\
                                        ZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBY\
                                        tgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ"]
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["credentials"][0]["credential"], json!(CREDENTIAL_JWT));

        if let Some(external_server) = external_server {
            // Assert that the event was dispatched to the target URL.
            assert!(external_server.received_requests().await.unwrap().len() == 1);
        }

        let notification_id = body.get("notification_id").and_then(|v| v.as_str()).unwrap();

        (access_token, notification_id.to_string())
    }

    #[rstest]
    #[case::pre_authorized_code(true, false, false, false, 0)]
    #[case::authorization_code(false, false, false, false, 0)]
    #[case::with_external_server(true, false, true, false, 0)]
    #[case::with_anonymous_access(true, true, false, false, 0)]
    #[case::with_external_server_and_self_signed_credential(true, false, true, true, 0)]
    #[should_panic(expected = "assertion `left == right` failed\n  left: 500\n right: 200")]
    #[case::should_panic_due_to_timeout(true, false, true, false, DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS + 100)]
    #[serial_test::serial]
    #[tokio::test(flavor = "multi_thread")]
    #[tracing_test::traced_test]
    async fn test_credential_endpoint(
        #[case] is_pre_authorized: bool,
        #[case] with_anonymous_access: bool,
        #[case] with_external_server: bool,
        #[case] is_self_signed: bool,
        #[case] delay: u64,
    ) {
        let (external_server, issuance_event_publishers) = if with_external_server {
            let external_server = MockServer::start().await;

            let target_url = format!("{}/ssi-events-subscriber", &external_server.uri());

            set_config().enable_event_publisher_http(0);
            set_config().set_event_publisher_http_target_url(0, target_url.clone());
            set_config().set_event_publisher_http_target_events(0, Events {
                offer: vec![agent_shared::config::OfferEvent::CredentialRequestVerified],
                ..Default::default()
            });

            (
                Some(external_server),
                EventPublisherHttp::load()
                    .unwrap()
                    .into_iter()
                    .map(|p| Box::new(p) as Box<dyn EventPublisher>)
                    .collect(),
            )
        } else {
            (None, Default::default())
        };

        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, issuance_event_publishers).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let command = agent_issuance::nonce::command::NonceCommand::GenerateNonce {
            c_nonce: TEST_NONCE.to_string(),
        };
        agent_shared::handlers::command_handler(TEST_NONCE, &issuance_state.command.nonce, command)
            .await
            .unwrap();

        let mut issuance_app = router(issuance_state.clone());

        if let Some(external_server) = &external_server {
            external_server
                .prepare_credential_event_trigger(
                    Arc::new(Mutex::new(Some(issuance_app.clone()))),
                    is_self_signed,
                    delay,
                )
                .await;
        }

        let credential_configuration_id = if is_pre_authorized {
            "001".to_string()
        } else {
            "002".to_string()
        };

        // When `with_external_server` is false, then the credentials endpoint does not need to be called before the
        // start of the flow, since the `external_server` will do this once it is triggered by the
        // `CredentialRequestVerified` event.
        if !with_external_server {
            credentials(&mut issuance_app, &credential_configuration_id).await;
        }

        let grants = offers(&mut issuance_app, &credential_configuration_id).await.unwrap();

        let authorization_state =
            Arc::new(authorization_state(&InMemory, AuthorizationServices::default().await, Default::default()).await);
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut authorization_app = authorization::router((authorization_state, issuance_state));

        let access_token: String = token(&mut authorization_app, is_pre_authorized, grants).await;
        let jwt = if with_anonymous_access {
            // This JWT has no nonce, so no need to pre-generate
            "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVTMjU2Iiwia2lkIjoiZGlkOmp3azpleUpoYkdjaU9pSkZVekkxTmlJc0ltTnlkaUk2SWxBdE1qVTJJaXdpYTJsa0lqb2lhMUoyYms1M1N6QlhSbTlWZEVGR1JEZDFSbGN6Y3pSbWVFbDBVVmRKZGpaTU9FRldYMEV0VTFOV2J5SXNJbXQwZVNJNklrVkRJaXdpZUNJNklrSXRWblp4V2xsMVUyVmlXbXBoWVRNMlExcGhaMnRKVFZWeU9ERlZRMjR0U0ROZlJXbHBYMnByUlRBaUxDSjVJam9pZW01SFgwVXhiWFZQT1dsWGNFbHdPWE16VUZWbWRXUnlZelpNV2pBdGEwNDJVREJrUm5neldFeDRXU0o5IzAifQ.eyJhdWQiOiJodHRwOi8vbG9jYWxob3N0OjMwMzMvIiwiaWF0IjoxNzcxNDE1OTg2LCJub25jZSI6IjdlMDNhZDNmNzZjYjMzMzhjM2E1NjQyZmU3NjM0NDc2YWEzYWQ5M2ZhMWQ1ODQwMTFiYTIxNTBkOWRhNDcxMzMifQ.1vmzQVFvo90TSp8Yh9CbqJHyrzjE3U5xQN4G8BGPk6-vlrWHejVPjk-oWW8uXRmdiRsmJjRCUeSN3fLKdlTK_g"
        } else {
            // This JWT contains the nonce we just generated
            "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBYtgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ"
        };

        let response = issuance_app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": credential_configuration_id,
                            "proofs": {
                                "jwt":[jwt]
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        if with_anonymous_access {
            assert_eq!(body["credentials"][0]["credential"], json!(ANONYMOUS_CREDENTIAL_JWT));
        } else {
            assert_eq!(body["credentials"][0]["credential"], json!(CREDENTIAL_JWT));
        }

        if let Some(external_server) = external_server {
            // Assert that the event was dispatched to the target URL.
            assert!(external_server.received_requests().await.unwrap().len() == 1);
        }
    }
}
