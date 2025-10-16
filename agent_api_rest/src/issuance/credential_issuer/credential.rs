use std::time::{Duration, Instant};

use crate::{
    handlers::{command_handler, query_handler},
    issuance::error::internal_server_error,
    issuance::error::PublicError,
};
use agent_issuance::{
    credential::{command::CredentialCommand, views::CredentialView},
    offer::{command::OfferCommand, views::OfferView},
    server_config::views::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::config::config;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use oid4vci::credential_request::CredentialRequest;
use oid4vci::errors::CredentialErrorResponse;
use tokio::time::sleep;
use tracing::error;

const POLLING_INTERVAL_MS: u64 = 100;

#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<IssuanceState>,
    AuthBearer(access_token): AuthBearer,
    Json(credential_request): Json<CredentialRequest>,
) -> Result<Response, PublicError> {
    // Use the `access_token` to get the `offer_id` from the `AccessTokenView`.
    let offer_id = query_handler(&access_token, &state.query.access_token)
        .await?
        .ok_or_else(|| PublicError::from(CredentialErrorResponse::InvalidToken))?
        .offer_id;

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
                subject_id: Some(subject_id),
                ..
            }) => break (credential_ids, subject_id),
            _ => {
                return Err(internal_server_error());
            }
        }
    };

    // Use the `credential_ids` and `subject_id` to sign all the credentials.
    let mut signed_credentials = vec![];
    for credential_id in credential_ids {
        let command = CredentialCommand::SignCredential {
            credential_id: credential_id.clone(),
            subject_id: subject_id.clone(),
            overwrite: false,
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
    use crate::issuance::credentials::tests::credentials;
    use crate::issuance::router;
    use crate::API_VERSION;
    use crate::{
        issuance::{
            credential_issuer::token::tests::token, credentials::CredentialsEndpointRequest, offers::tests::offers,
        },
        tests::{CREDENTIAL_CONFIGURATION_ID, OFFER_ID},
    };
    use agent_event_publisher_http::EventPublisherHttp;
    use agent_issuance::credential::aggregate::CredentialExpiry;
    use agent_issuance::{offer::event::OfferEvent, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_shared::config::{set_config, Events};
    use agent_store::{in_memory, EventPublisher};
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

    const CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIl0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsiaWQiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4ifSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsImNyZWRlbnRpYWxTdGF0dXMiOnsiaWQiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAiLCJ0eXBlIjoic3RhdHVzbGlzdCtqd3QiLCJpZHgiOjEyMywidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIn19LCJzdGF0dXMiOnsic3RhdHVzX2xpc3QiOnsiaWR4IjoxMjMsInVyaSI6Imh0dHBzOi8vbXktZG9tYWluLmV4YW1wbGUub3JnL2lldGYtb2F1dGgtdG9rZW4tc3RhdHVzLWxpc3QvMCJ9fX0.LpNq8l-qqqCA-htsB8KZLaVoNCfxqTrsPxVmEj0dsPAGFhOqO8lXI7DU0FhNwzWedxJ1ySS_Vq7ChBW-TgY7Bw";
    const DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS: u64 = 1000;

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
                                        expires_at: CredentialExpiry::Never,
                                        delivery_options: None,
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
                                        expires_at: CredentialExpiry::Never,
                                        delivery_options: None,
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

    #[rstest]
    #[case::without_external_server(false, false, 0)]
    #[case::with_external_server(true, false, 0)]
    #[case::with_external_server_and_self_signed_credential(true, true, 0)]
    #[should_panic(expected = "assertion `left == right` failed\n  left: 500\n right: 200")]
    #[case::should_panic_due_to_timeout(true, false, DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS + 100)]
    #[serial_test::serial]
    #[tokio::test(flavor = "multi_thread")]
    #[tracing_test::traced_test]
    async fn test_credential_endpoint(
        #[case] with_external_server: bool,
        #[case] is_self_signed: bool,
        #[case] delay: u64,
    ) {
        let (external_server, issuance_event_publishers) = if with_external_server {
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
            )
        } else {
            (None, Default::default())
        };

        let issuance_state = in_memory::issuance_state(Service::default(), issuance_event_publishers).await;
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

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

        let pre_authorized_code = offers(&mut app).await.unwrap();

        let access_token: String = token(&mut app, pre_authorized_code).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": CREDENTIAL_CONFIGURATION_ID,
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
        assert_eq!(body["credentials"][0]["credential"], json!(CREDENTIAL_JWT));

        if let Some(external_server) = external_server {
            // Assert that the event was dispatched to the target URL.
            assert!(external_server.received_requests().await.unwrap().len() == 1);
        }
    }

    pub async fn credential(app: &mut Router) -> (String, String) {
        credentials(app).await;
        let pre_authorized_code = offers(app).await.unwrap();
        let access_token: String = token(app, pre_authorized_code).await;

        let request_body = json!({
            "credential_configuration_id": CREDENTIAL_CONFIGURATION_ID,
                "proof": {
                    "proof_type": "jwt",
                    "jwt": "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBY\
                                    tgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ"
                }
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_value: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_value["credentials"][0]["credential"], json!(CREDENTIAL_JWT));

        let notification_id = body_value
            .get("notification_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        (access_token, notification_id.to_string())
    }
}
