use std::time::{Duration, Instant};

use crate::{
    handlers::{public_command_handler, public_query_handler},
    v0::{
        issuance::{
            credential_issuer::protocol_request,
            error::{internal_server_error, PublicError},
            public_offers::is_public_offer,
        },
        openapi::PROTOCOL_TAG,
    },
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
    status_list::application::index_allocation::{add_index, status_list_with_room},
};
use agent_shared::config::config;
use axum::{
    extract::{rejection::JsonRejection, Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use oid4vci::credential_request::CredentialRequest;
use oid4vci::errors::CredentialErrorResponse;
use oid4vci::{credential_response::CredentialResponse, errors::OID4VCError};
use std::sync::Arc;
use tokio::time::sleep;
use tracing::error;

const POLLING_INTERVAL_MS: u64 = 100;

/// Issue credentials
///
/// Issues the credentials of an offer in exchange for a valid access token and key proofs, as defined by
/// [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-credential-endpoint).
#[utoipa::path(
    post,
    path = "/openid4vci/credential",
    operation_id = "openid4vci_credential",
    tags = ["OpenID4VCI", PROTOCOL_TAG],
    security(("access_token" = [])),
    request_body(content = CredentialRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Credentials issued", body = CredentialResponse),
        (
            status = 400,
            description = "The credential request is invalid",
            body = OID4VCError<CredentialErrorResponse>,
        ),
        (
            status = 401,
            description = "The access token is missing (`WWW-Authenticate: Bearer`, empty body), or invalid or expired",
            content_type = "application/json",
            headers(("WWW-Authenticate" = String, description = "`Bearer` without an access token, otherwise `Bearer error=\"invalid_token\"`")),
            example = json!({"error": "invalid_token"}),
        ),
        (status = 415, description = "The request body is not `application/json`"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential(
    State(state): State<Arc<IssuanceState>>,
    bearer: Result<AuthBearer, axum_auth::Rejection>,
    credential_request: Result<Json<CredentialRequest>, JsonRejection>,
) -> Result<Response, PublicError> {
    let (access_token, credential_request) = protocol_request(bearer, credential_request, || {
        CredentialErrorResponse::InvalidCredentialRequest.into()
    })?;
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
        match public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
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
    let is_public_offer = is_public_offer(&state, &offer_id).await?;

    let command = OfferCommand::VerifyCredentialRequest {
        offer_id: offer_id.clone(),
        credential_issuer_metadata,
        authorization_server_metadata,
        credential_request,
        is_public_offer,
    };

    // Use the `offer_id` to verify the `proof` inside the `CredentialRequest`.
    public_command_handler(&offer_id, &state.command.offer, command).await?;

    let timeout = config().external_server_response_timeout_ms;
    let start_time = Instant::now();

    // TODO: replace this polling solution with a call to the `TxChannelRegistry` as described here: https://github.com/impierce/ssi-agent/issues/75
    // Use the `offer_id` to get the `credential_ids` and `subject_id` from the `OfferView`.
    let (credential_ids, subject_id) = loop {
        match public_query_handler(&offer_id, &state.query.offer).await? {
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

    // Find a status list which is not full
    // TODO: this contains quite some domain logic and should actually be moved there.
    let status_list_id = status_list_with_room(&state, credential_ids.len())
        .await
        .map_err(|_| PublicError::InternalServerError)?;

    // Use the `credential_ids` and `subject_id` to sign all the credentials.
    let mut signed_credentials = vec![];
    for credential_id in credential_ids {
        let index = add_index(&state, &status_list_id)
            .await
            .map_err(|_| PublicError::InternalServerError)?;

        // The credentials of a Public Offer are shared by all its holders: each one gets them signed for itself.
        let command = CredentialCommand::SignCredential {
            credential_id: credential_id.clone(),
            subject_id: subject_id.clone(),
            overwrite: is_public_offer,
            proofs: proofs.clone(),
            status_list_id: status_list_id.clone(),
            index,
        };

        public_command_handler(&credential_id, &state.command.credential, command).await?;

        let signed_credential = match public_query_handler(&credential_id, &state.query.credential).await? {
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
    public_command_handler(&offer_id, &state.command.offer, command).await?;

    // Use the `offer_id` to get the `credential_response` from the `OfferView`.
    public_query_handler(&offer_id, &state.query.offer)
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
    use crate::v0::issuance::{
        credentials::tests::{create_test_template_with_auth, credentials_with_template, setup_library_state},
        router,
    };
    use crate::API_VERSION;
    use crate::{
        tests::{holder_did, key_proof, OFFER_ID},
        v0::issuance::{credentials::CredentialsEndpointRequest, offers::tests::offers},
    };

    use agent_authorization::authorization_state;
    use agent_authorization::services::AuthorizationServices;
    use agent_issuance::credential::aggregate::CredentialExpiry;
    use agent_issuance::issuance_state;
    use agent_issuance::offer::event::OfferEvent;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::IssuanceState;
    use agent_secret_manager::service::Service;
    use agent_shared::config::{set_config, Events};
    use agent_store::in_memory::InMemory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::utils::jwt::get_unverified_jwt_claims;
    use rstest::rstest;
    use serde_json::{json, Value};
    use shared_kernel::authorization::Caller;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6InVybjp1dWlkOjEyM2U0NTY3LWU4OWItMTJkMy1hNDU2LTQyNjYxNDE3NDAwMCIsInZjIjp7ImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImZpcnN0X25hbWUiOiJGZXJyaXMiLCJsYXN0X25hbWUiOiJSdXN0YWNlYW4iLCJpZCI6ImRpZDprZXk6ejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0sInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiXSwiaWQiOiJ1cm46dXVpZDoxMjNlNDU2Ny1lODliLTEyZDMtYTQ1Ni00MjY2MTQxNzQwMDAiLCJuYW1lIjoiVmVyaWZpYWJsZSBDcmVkZW50aWFsIiwiaXNzdWVyIjp7Im5hbWUiOiJVbmlDb3JlIiwiaWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9LCJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIseyJsb2dvX3VyaSI6eyJAaWQiOiJodHRwczovL3d3dy5pYW5hLm9yZy9hc3NpZ25tZW50cy9qd3QjbG9nb191cmkiLCJAdHlwZSI6IkBpZCJ9fV0sImlzc3VhbmNlRGF0ZSI6IjIwMTAtMDEtMDFUMDA6MDA6MDBaIiwidmFsaWRGcm9tIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJjcmVkZW50aWFsU3RhdHVzIjp7InR5cGUiOiJzdGF0dXNsaXN0K2p3dCIsImlkIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwidXJpIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvaWV0Zi1vYXV0aC10b2tlbi1zdGF0dXMtbGlzdC8wIiwiaWR4IjoxMjN9fSwic3RhdHVzIjp7InN0YXR1c19saXN0Ijp7ImlkeCI6MTIzLCJ1cmkiOiJodHRwczovL215LWRvbWFpbi5leGFtcGxlLm9yZy9pZXRmLW9hdXRoLXRva2VuLXN0YXR1cy1saXN0LzAifX19.COxDXhB4zmiP79bU2wVGCWXMjk24z38vCjC8j23sXjaEhzvsCr_v_HuhPDVh3pkNzcF09tULDZiwjg0tgM19Cw";
    const DEFAULT_EXTERNAL_SERVER_RESPONSE_TIMEOUT_MS: u64 = 1000;
    pub const TEST_NONCE: &str = "7e03ad3f76cb3338c3a5642fe7634476aa3ad93fa1d584011ba2150d9da47133";

    /// The claims of `CREDENTIAL_JWT`, but for the test holder's `did:key`: the DID of the key of its proofs.
    async fn credential_jwt_claims_for_holder() -> Value {
        let claims = get_unverified_jwt_claims(&json!(CREDENTIAL_JWT))
            .unwrap()
            .to_string()
            .replace(
                "did:key:z6MkiieyoLMSVsJAZv7Jje5wWSkDEymUgkyF8kbcrjZpX3qd",
                &holder_did("did:key").await,
            );
        serde_json::from_str(&claims).unwrap()
    }

    trait CredentialEventTrigger {
        async fn prepare_credential_event_trigger(
            &self,
            app: Arc<Mutex<Option<Router>>>,
            template_id: String,
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
            template_id: String,
            is_self_signed: bool,
            delay: u64,
        ) {
            Mock::given(method("POST"))
                .and(path("/ssi-events-subscriber"))
                .and(move |request: &wiremock::Request| {
                    let cloud_event: shared_kernel::event_bus::CloudEvent = request.body_json().unwrap();
                    let data = cloud_event.data.unwrap();
                    let offer_event: OfferEvent = serde_json::from_value(serde_json::json!({
                        "CredentialRequestVerified": data
                    }))
                    .unwrap();
                    match offer_event {
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
                                        template_id: template_id.clone(),
                                        offer_id: offer_id.clone(),
                                        credential: json!(CREDENTIAL_JWT),
                                        is_signed: true,
                                        expires_at: Some(CredentialExpiry::Never),
                                    }
                                } else {
                                    // ...or else, submitting the data that will be signed inside `UniCore`.
                                    CredentialsEndpointRequest {
                                        template_id: template_id.clone(),
                                        offer_id: offer_id.clone(),
                                        credential: json!({
                                            "credentialSubject": {
                                                "first_name": "Ferris",
                                                "last_name": "Rustacean",
                                                "id": subject_id
                                            }
                                        }),
                                        is_signed: false,
                                        expires_at: Some(CredentialExpiry::Never),
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
                    }
                })
                .respond_with(ResponseTemplate::new(200))
                .mount(self)
                .await;
        }
    }

    pub async fn credential(
        issuance_app: &mut Router,
        issuance_state: &Arc<IssuanceState>,
        template_id: &str,
        access_token: String,
        external_server: Option<MockServer>,
    ) -> (String, String) {
        let command = agent_issuance::nonce::command::NonceCommand::GenerateNonce {
            c_nonce: TEST_NONCE.to_string(),
        };
        agent_shared::handlers::command_handler(
            issuance_state.authorization_checker.clone(),
            Caller::Internal,
            TEST_NONCE,
            &issuance_state.command.nonce,
            command,
        )
        .await
        .unwrap();

        let proof = key_proof("did:key", Some(holder_did("did:key").await), TEST_NONCE).await;

        let response = issuance_app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/openid4vci/credential")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": template_id,
                            "proofs": { "jwt": [proof] }
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
            get_unverified_jwt_claims(&body["credentials"][0]["credential"]).unwrap(),
            credential_jwt_claims_for_holder().await
        );

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
        let (external_server, bus_opt) = if with_external_server {
            let external_server = MockServer::start().await;

            let target_url = format!("{}/ssi-events-subscriber", &external_server.uri());

            set_config().enable_event_publisher_http(0);
            set_config().set_event_publisher_http_target_url(0, target_url.clone());
            set_config().set_event_publisher_http_target_events(
                0,
                Events {
                    types: vec!["CredentialRequestVerified".to_string()],
                },
            );

            let bus = shared_kernel::event_bus::EventBusHandle::new(1024);
            if let Some(publisher) = agent_event_publisher_http::HttpEventPublisher::from_config(&bus) {
                publisher.spawn();
            }
            (Some(external_server), Some(bus))
        } else {
            (None, None)
        };

        let event_bus = bus_opt.unwrap_or_default();
        let issuance_state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &event_bus).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template_with_auth(&library_state, is_pre_authorized).await;

        let command = agent_issuance::nonce::command::NonceCommand::GenerateNonce {
            c_nonce: TEST_NONCE.to_string(),
        };
        agent_shared::handlers::command_handler(
            issuance_state.authorization_checker.clone(),
            Caller::Internal,
            TEST_NONCE,
            &issuance_state.command.nonce,
            command,
        )
        .await
        .unwrap();

        let mut issuance_app = router((issuance_state.clone(), library_state));

        if let Some(external_server) = &external_server {
            external_server
                .prepare_credential_event_trigger(
                    Arc::new(Mutex::new(Some(issuance_app.clone()))),
                    template_id.clone(),
                    is_self_signed,
                    delay,
                )
                .await;
        }

        // When `with_external_server` is false, then the credentials endpoint does not need to be called before the
        // start of the flow, since the `external_server` will do this once it is triggered by the
        // `CredentialRequestVerified` event.
        if !with_external_server {
            credentials_with_template(&mut issuance_app, &template_id).await;
        }

        let grants = offers(&mut issuance_app, &template_id).await.unwrap();

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &event_bus,
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut authorization_app = authorization::router((authorization_state, issuance_state));

        let access_token: String = token(&mut authorization_app, is_pre_authorized, grants).await;
        let jwt = if with_anonymous_access {
            // With anonymous access to the token endpoint, the proof has no `iss` (OpenID4VCI 1.0, Appendix F.1).
            key_proof("did:jwk", None, TEST_NONCE).await
        } else {
            key_proof("did:key", Some(holder_did("did:key").await), TEST_NONCE).await
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
                            "credential_configuration_id": template_id,
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
            // Without `iss`, the credential is still bound to the holder: the DID of the key of its proof.
            let credential = get_unverified_jwt_claims(&body["credentials"][0]["credential"]).unwrap();
            assert_eq!(credential["sub"], json!(holder_did("did:jwk").await));
        } else if is_self_signed {
            assert_eq!(body["credentials"][0]["credential"], json!(CREDENTIAL_JWT));
        } else {
            assert_eq!(
                get_unverified_jwt_claims(&body["credentials"][0]["credential"]).unwrap(),
                credential_jwt_claims_for_holder().await
            );
        }

        if let Some(external_server) = external_server {
            // Assert that the event was dispatched to the target URL.
            assert!(external_server.received_requests().await.unwrap().len() == 1);
        }
    }

    /// A fresh nonce for a key proof: each Credential Request redeems its own.
    async fn generate_nonce(issuance_state: &IssuanceState) -> String {
        let c_nonce = agent_shared::generate_random_string();
        agent_shared::handlers::command_handler(
            issuance_state.authorization_checker.clone(),
            Caller::Internal,
            &c_nonce,
            &issuance_state.command.nonce,
            agent_issuance::nonce::command::NonceCommand::GenerateNonce {
                c_nonce: c_nonce.clone(),
            },
        )
        .await
        .unwrap();
        c_nonce
    }

    #[rstest]
    #[case::credential_offer(false)]
    #[case::public_offer(true)]
    #[serial_test::serial]
    #[tokio::test(flavor = "multi_thread")]
    async fn credential_offer_belongs_to_holder_of_first_verified_request(#[case] is_public_offer: bool) {
        let event_bus = shared_kernel::event_bus::EventBusHandle::default();
        let issuance_state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &event_bus).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        let template_id = create_test_template_with_auth(&library_state, true).await;

        let mut issuance_app = router((issuance_state.clone(), library_state));
        credentials_with_template(&mut issuance_app, &template_id).await;
        let grants = offers(&mut issuance_app, &template_id).await.unwrap();

        if is_public_offer {
            crate::handlers::public_command_handler(
                &format!("public_offer:{OFFER_ID}"),
                &issuance_state.command.public_offer,
                agent_issuance::public_offer::command::PublicOfferCommand::Create {
                    offer_id: OFFER_ID.to_string(),
                    template_id: template_id.clone(),
                },
            )
            .await
            .unwrap();
        }

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &event_bus,
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();
        let mut authorization_app = authorization::router((authorization_state, issuance_state.clone()));
        let access_token: String = token(&mut authorization_app, true, grants).await;

        let request_credential = |did_method: &'static str, iss: Option<String>| {
            let issuance_app = issuance_app.clone();
            let issuance_state = issuance_state.clone();
            let access_token = access_token.clone();
            let template_id = template_id.clone();
            async move {
                let nonce = generate_nonce(&issuance_state).await;
                let proof = key_proof(did_method, iss, &nonce).await;
                let response = issuance_app
                    .oneshot(
                        Request::builder()
                            .method(http::Method::POST)
                            .uri("/openid4vci/credential")
                            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                            .header(http::header::AUTHORIZATION, format!("Bearer {access_token}"))
                            .body(Body::from(
                                serde_json::to_vec(&json!({
                                    "credential_configuration_id": template_id,
                                    "proofs": { "jwt": [proof] }
                                }))
                                .unwrap(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
                (status, serde_json::from_slice::<Value>(&body).unwrap_or_default())
            }
        };

        let holder = holder_did("did:key").await;
        let other_holder = holder_did("did:jwk").await;
        let subject =
            |body: &Value| get_unverified_jwt_claims(&body["credentials"][0]["credential"]).unwrap()["sub"].clone();

        // The first holder redeems the offer.
        let (status, body) = request_credential("did:key", Some(holder.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(subject(&body), json!(holder));

        // Another holder (the `did:jwk` key), also when its proof claims the first holder's DID as `iss`. A Public
        // Offer issues to it a credential of its own.
        for iss in [None, Some(holder.clone())] {
            let (status, body) = request_credential("did:jwk", iss).await;
            if is_public_offer {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(subject(&body), json!(other_holder));
            } else {
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert_eq!(body["error"], "credential_request_denied");
            }
        }

        // The first holder again, also without `iss`.
        let (status, body) = request_credential("did:key", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(subject(&body), json!(holder));
    }
}
