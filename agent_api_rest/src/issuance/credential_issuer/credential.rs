use std::time::{Duration, Instant};

use agent_issuance::{
    credential::{command::CredentialCommand, queries::CredentialView},
    offer::{
        command::OfferCommand,
        queries::{access_token::AccessTokenView, OfferView},
    },
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::{
    config::config,
    handlers::{command_handler, query_handler},
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use oid4vci::credential_request::CredentialRequest;
use serde_json::json;
use tokio::time::sleep;
use tracing::{error, info};

const DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS: u64 = 1000;
const POLLING_INTERVAL_MS: u64 = 100;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<IssuanceState>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
    // TODO: implement official oid4vci error response. This TODO is also in the `token` endpoint.
) -> Response {
    info!("Request Body: {}", json!(credential_request));

    // Use the `access_token` to get the `offer_id` from the `AccessTokenView`.
    let offer_id = match query_handler(&access_token, &state.query.access_token).await {
        Ok(Some(AccessTokenView { offer_id })) => offer_id,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };

    // Get the `credential_issuer_metadata` and `authorization_server_metadata` from the `ServerConfigView`.
    let (credential_issuer_metadata, authorization_server_metadata) =
        match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
            Ok(Some(ServerConfigView {
                credential_issuer_metadata: Some(credential_issuer_metadata),
                authorization_server_metadata,
            })) => (credential_issuer_metadata, Box::new(authorization_server_metadata)),
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

    let command = OfferCommand::VerifyCredentialRequest {
        offer_id: offer_id.clone(),
        credential_issuer_metadata,
        authorization_server_metadata,
        credential_request,
    };

    // Use the `offer_id` to verify the `proof` inside the `CredentialRequest`.
    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let timeout = config()
        .external_server_response_timeout_ms
        .unwrap_or(DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS);
    let start_time = Instant::now();

    // TODO: replace this polling solution with a call to the `TxChannelRegistry` as described here: https://github.com/impierce/ssi-agent/issues/75
    // Use the `offer_id` to get the `credential_ids` and `subject_id` from the `OfferView`.
    let (credential_ids, subject_id) = loop {
        match query_handler(&offer_id, &state.query.offer).await {
            // When the Offer does not include the credential id's yet, wait for the external server to provide them.
            Ok(Some(OfferView { credential_ids, .. })) if credential_ids.is_empty() => {
                if start_time.elapsed().as_millis() <= timeout.into() {
                    sleep(Duration::from_millis(POLLING_INTERVAL_MS)).await;
                } else {
                    error!("timeout failure");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
            Ok(Some(OfferView {
                credential_ids,
                subject_id: Some(subject_id),
                ..
            })) => break (credential_ids, subject_id),
            _ => {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    // Use the `credential_ids` and `subject_id` to sign all the credentials.
    let mut signed_credentials = vec![];
    for credential_id in credential_ids {
        let command = CredentialCommand::SignCredential {
            subject_id: subject_id.clone(),
            overwrite: false,
        };

        if command_handler(&credential_id, &state.command.credential, command)
            .await
            .is_err()
        {
            StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };

        let signed_credential = match query_handler(&credential_id, &state.query.credential).await {
            Ok(Some(CredentialView {
                signed: Some(signed_credential),
                ..
            })) => signed_credential,
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

        signed_credentials.push(signed_credential);
    }

    let command = OfferCommand::CreateCredentialResponse {
        offer_id: offer_id.clone(),
        signed_credentials,
    };

    // Use the `offer_id` to create a `CredentialResponse` from the `CredentialRequest` and `credentials`.
    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Use the `offer_id` to get the `credential_response` from the `OfferView`.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView {
            credential_response: Some(credential_response),
            ..
        })) => (StatusCode::OK, Json(credential_response)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        app,
        issuance::{
            credential_issuer::token::tests::token, credentials::CredentialsEndpointRequest, offers::tests::offers,
        },
        tests::{BASE_URL, CREDENTIAL_CONFIGURATION_ID, OFFER_ID},
    };

    use super::*;
    use crate::issuance::credentials::tests::credentials;
    use crate::API_VERSION;
    use agent_event_publisher_http::EventPublisherHttp;
    use agent_issuance::services::test_utils::test_issuance_services;
    use agent_issuance::{offer::event::OfferEvent, startup_commands::startup_commands, state::initialize};
    use agent_shared::config::{set_config, Events};
    use agent_store::{in_memory, EventPublisher};
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use rstest::rstest;
    use serde_json::{json, Value};
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjoiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQiLCJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIn0sImlzc3VlciI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoifX0.d4QN73vDtZu79RP6GldHObu6rGsjidkLYp0XMRQNbNPY75LJoSv2iXk2Rz5M-VMBZGSU3YPZHytlrKBjxr1IBQ";

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
                                        credential_configuration_id: CREDENTIAL_CONFIGURATION_ID.to_string(),
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
                                        credential_configuration_id: CREDENTIAL_CONFIGURATION_ID.to_string(),
                                    }
                                };

                                std::thread::sleep(Duration::from_millis(delay));

                                // Sends the `CredentialsRequest` to the `credentials` endpoint.
                                app_clone
                                    .oneshot(
                                        Request::builder()
                                            .method(http::Method::POST)
                                            .uri(&format!("{API_VERSION}/credentials"))
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

    #[rstest]
    #[case::without_external_server(false, false, 0)]
    #[case::with_external_server(true, false, 0)]
    #[case::with_external_server_and_self_signed_credential(true, true, 0)]
    #[should_panic(expected = "assertion `left == right` failed\n  left: 500\n right: 200")]
    #[case::should_panic_due_to_timout(true, false, DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS + 100)]
    #[serial_test::serial]
    #[tokio::test(flavor = "multi_thread")]
    #[tracing_test::traced_test]
    async fn test_credential_endpoint(
        #[case] with_external_server: bool,
        #[case] is_self_signed: bool,
        #[case] delay: u64,
    ) {
        let (external_server, issuance_event_publishers, verification_event_publishers) = if with_external_server {
            let external_server = MockServer::start().await;

            let target_url = format!("{}/ssi-events-subscriber", &external_server.uri());

            set_config().enable_event_publisher_http();
            set_config().set_event_publisher_http_target_url(target_url.clone());
            set_config().set_event_publisher_http_target_events(Events {
                offer: vec![agent_shared::config::OfferEvent::CredentialRequestVerified],
                ..Default::default()
            });

            (
                Some(external_server),
                vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn EventPublisher>],
                vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn EventPublisher>],
            )
        } else {
            (None, Default::default(), Default::default())
        };

        let issuance_state = in_memory::issuance_state(test_issuance_services(), issuance_event_publishers).await;
        let verification_state =
            in_memory::verification_state(test_verification_services(), verification_event_publishers).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = app((issuance_state, verification_state));

        if let Some(external_server) = &external_server {
            external_server
                .prepare_credential_event_trigger(Arc::new(Mutex::new(Some(app.clone()))), is_self_signed, delay)
                .await;
        }

        // When `with_external_server` is false, then the credentials endpoint does not need to be called before the
        // start of the flow, since the `external_server` will do this once it is triggered by the
        // `CredentialRequestVerified` event.
        if !with_external_server {
            credentials(&mut app).await;
        }

        let pre_authorized_code = offers(&mut app).await;

        let access_token = token(&mut app, pre_authorized_code).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {}", access_token))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "format": "jwt_vc_json",
                            "credential_definition": {
                                "type": [
                                    "VerifiableCredential",
                                    "OpenBadgeCredential"
                                ]
                            },
                            "proof": {
                                "proof_type": "jwt",
                                "jwt": "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lk\
                                        IjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4\
                                        a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lG\
                                        OGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFa\
                                        djdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8v\
                                        ZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAs\
                                        Im5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkz\
                                        ZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBY\
                                        tgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ"
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
        assert_eq!(
            body,
            json!({
                    "credential": CREDENTIAL_JWT
                }
            )
        );

        if let Some(external_server) = external_server {
            // Assert that the event was dispatched to the target URL.
            assert!(external_server.received_requests().await.unwrap().len() == 1);
        }
    }
}
