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
    config,
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

const DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS: u128 = 1000;

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

    let timeout = config!("external_server_response_timeout_ms")
        .ok()
        .and_then(|external_server_response_timeout_ms| external_server_response_timeout_ms.parse().ok())
        .unwrap_or(DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS);
    let start_time = Instant::now();

    // Use the `offer_id` to get the `credential_ids` and `subject_id` from the `OfferView`.
    let (credential_ids, subject_id) = loop {
        match query_handler(&offer_id, &state.query.offer).await {
            // When the Offer does not include the credential id's yet, wait for the external server to provide them.
            Ok(Some(OfferView { credential_ids, .. })) if credential_ids.is_empty() => {
                if start_time.elapsed().as_millis() <= timeout {
                    sleep(Duration::from_millis(50)).await;
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
        tests::{BASE_URL, OFFER_ID},
    };

    use super::*;
    use agent_event_publisher_http::{EventPublisherHttp, TEST_EVENT_PUBLISHER_HTTP_CONFIG};
    use agent_issuance::{offer::event::OfferEvent, startup_commands::startup_commands, state::initialize};
    use agent_shared::config;
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

    const CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dF\
                                  ODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtn\
                                  RTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJ\
                                  pc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXd\
                                  KdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp\
                                  2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwiZXhwIjo5OTk5OTk5OTk\
                                  5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8\
                                  yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmc\
                                  vc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9\
                                  leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZmlhYmx\
                                  lQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGl\
                                  kOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05\
                                  rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5\
                                  hbWUiOiJUZWFtd29yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImZpcnN\
                                  0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJpZCI6ImR\
                                  pZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3J\
                                  qWnBYM3FkIn19fQ.MzHsluxKNsnA01df0kUyXVBIzkBJajKhHbuG-_vNGz0QAPQ1\
                                  6jZ4IwAtEwt6XfbV9luFalRL3qtsmDvaNBf7CA";

    trait CredentialEventTrigger {
        async fn prepare_credential_event_trigger(
            &self,
            app: Arc<Mutex<Option<Router>>>,
            is_self_signed: bool,
            delay: u128,
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
            delay: u128,
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
                                    }
                                };

                                std::thread::sleep(Duration::from_millis(delay.try_into().unwrap()));

                                // Sends the `CredentialsRequest` to the `credentials` endpoint.
                                app_clone
                                    .oneshot(
                                        Request::builder()
                                            .method(http::Method::POST)
                                            .uri("/v1/credentials")
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
    // #[tracing_test::traced_test]
    async fn test_credential_endpoint(
        #[case] with_external_server: bool,
        #[case] is_self_signed: bool,
        #[case] delay: u128,
    ) {
        use crate::issuance::credentials::tests::credentials;

        let (external_server, issuance_event_publishers, verification_event_publishers) = if with_external_server {
            let external_server = MockServer::start().await;

            let target_url = format!("{}/ssi-events-subscriber", &external_server.uri());

            TEST_EVENT_PUBLISHER_HTTP_CONFIG.lock().unwrap().replace(
                serde_yaml::from_str(&format!(
                    r#"
                        target_url: &target_url {target_url}
    
                        offer: {{
                            target_url: *target_url,
                            target_events: [
                                CredentialRequestVerified
                            ]
                        }}
                    "#,
                ))
                .unwrap(),
            );

            (
                Some(external_server),
                vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn EventPublisher>],
                vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn EventPublisher>],
            )
        } else {
            (None, Default::default(), Default::default())
        };

        let issuance_state = in_memory::issuance_state(issuance_event_publishers).await;
        let verification_state = in_memory::verification_state(
            test_verification_services(&config!("default_did_method").unwrap_or("did:key".to_string())),
            verification_event_publishers,
        )
        .await;
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
            .expect("hello");

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
