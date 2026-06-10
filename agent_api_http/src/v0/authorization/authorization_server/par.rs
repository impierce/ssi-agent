use crate::{utils::StringifiedForm, v0::issuance::error::PublicError};
use agent_authorization::application::{
    interactive_authorization_service::InteractiveAuthorizationService,
    pushed_authorization_service::PushedAuthorizationService,
};
use agent_authorization::state::AuthorizationState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use oid4vci::{authorization_request::AuthorizationRequest, InteractiveAuthorizationRequest};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum AuthorizationRequestDto {
    InteractiveAuthorizationRequest(InteractiveAuthorizationRequest),
    FollowUpInteractiveAuthorizationRequest {
        auth_session: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        openid4vp_response: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code_verifier: Option<String>,
    },
    PushedAuthorizationRequest(AuthorizationRequest),
}

/// Handles the Pushed Authorization Request (PAR) endpoint as well as the Interactive Authorization Request flow as defined in OpenID4VCI 1.1
#[axum_macros::debug_handler]
pub(crate) async fn par(
    State(state): State<Arc<AuthorizationState>>,
    StringifiedForm(authorization_request): StringifiedForm<AuthorizationRequestDto>,
) -> Result<Response, PublicError> {
    match authorization_request {
        AuthorizationRequestDto::InteractiveAuthorizationRequest(interactive_authorization_request) => {
            info!("Received interactive authorization request");

            let interactive_authorization_response =
                InteractiveAuthorizationService::handle_interactive_authorization_request(
                    &state,
                    interactive_authorization_request,
                )
                .await
                // TODO: implement proper error handling
                .map_err(|_err| PublicError::InternalServerError)?;

            Ok((StatusCode::OK, Json(interactive_authorization_response)).into_response())
        }
        AuthorizationRequestDto::FollowUpInteractiveAuthorizationRequest {
            auth_session,
            openid4vp_response,
            code_verifier,
        } => {
            info!("Received follow-up interactive authorization request for auth session: {auth_session}");

            let interactive_authorization_follow_up_response =
                InteractiveAuthorizationService::handle_interactive_authorization_request_follow_up(
                    &state,
                    auth_session,
                    openid4vp_response,
                    code_verifier,
                )
                .await
                // TODO: implement proper error handling
                .map_err(|_err| PublicError::InternalServerError)?;

            Ok((StatusCode::OK, Json(interactive_authorization_follow_up_response)).into_response())
        }
        AuthorizationRequestDto::PushedAuthorizationRequest(pushed_authorization_request) => {
            info!(
                "Received pushed authorization request with state: {:?}",
                pushed_authorization_request.state
            );

            let authorization_response =
                PushedAuthorizationService::handle_pushed_authorization_request(&state, pushed_authorization_request)
                    .await
                    // TODO: implement proper error handling
                    .map_err(|_err| PublicError::InternalServerError)?;

            Ok((StatusCode::CREATED, Json(authorization_response)).into_response())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::v0::{
        authorization,
        issuance::{self, credentials::tests::credentials, offers::tests::offers},
    };
    use agent_authorization::{
        domain::oauth2_authorization_request::aggregate::test_utils::code_challenge, state::UNIME_REDIRECT_URI,
    };
    use agent_authorization::{services::AuthorizationServices, state::UNIME_CLIENT_ID};
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::utils::form_urlencoded::to_form_urlencoded_string;
    use oid4vci::{
        authorization_details::{AuthorizationDetailsObject, OpenidCredential},
        authorization_request::CodeChallengeMethod,
        credential_offer::AuthorizationCode,
        wallet::PushedAuthorizationResponse,
        InteractiveAuthorizationResponse,
    };
    use serde_json::json;
    use tower::Service as _;

    pub async fn par(app: &mut Router, issuer_state: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(AuthorizationRequest {
                            response_type: "code".to_string(),
                            state: Some("test_state".to_string()),
                            client_id: UNIME_CLIENT_ID.to_string(),
                            redirect_uri: Some(UNIME_REDIRECT_URI.parse().unwrap()),
                            code_challenge: Some(code_challenge()),
                            code_challenge_method: Some(CodeChallengeMethod::S256),
                            scope: Some("openid profile".to_string()),
                            issuer_state: Some(issuer_state),
                            authorization_details: Some(vec![AuthorizationDetailsObject {
                                r#type: OpenidCredential::Type,
                                locations: None,
                                credential_configuration_id: "configuration_id".to_string(),
                                credential_identifiers: None,
                                claims: None,
                            }]),
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let pushed_authorization_response: PushedAuthorizationResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(pushed_authorization_response.expires_in, 3600);

        pushed_authorization_response.request_uri
    }

    pub async fn interactive_authorization_request(
        app: &mut Router,
        issuer_state: String,
    ) -> InteractiveAuthorizationResponse {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(InteractiveAuthorizationRequest {
                            authorization_request: AuthorizationRequest {
                                response_type: "code".to_string(),
                                state: Some("test_state".to_string()),
                                client_id: UNIME_CLIENT_ID.to_string(),
                                redirect_uri: Some(UNIME_REDIRECT_URI.parse().unwrap()),
                                code_challenge: Some(code_challenge()),
                                code_challenge_method: Some(CodeChallengeMethod::S256),
                                scope: None,
                                issuer_state: Some(issuer_state),
                                authorization_details: Some(vec![AuthorizationDetailsObject {
                                    r#type: OpenidCredential::Type,
                                    locations: None,
                                    credential_configuration_id: "configuration_id".to_string(),
                                    credential_identifiers: None,
                                    claims: None,
                                }]),
                            },
                            interaction_types_supported: "FIXME".to_string(),
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
        let interactive_authorization_response: InteractiveAuthorizationResponse =
            serde_json::from_slice(&body).unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(
                            AuthorizationRequestDto::FollowUpInteractiveAuthorizationRequest {
                                auth_session: interactive_authorization_response.auth_session.clone().unwrap(),
                                openid4vp_response: Some(serde_json::json!({})),
                                code_verifier: None,
                            }
                        ))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let interactive_authorization_response: InteractiveAuthorizationResponse =
            serde_json::from_slice(&body).unwrap();

        interactive_authorization_response
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_pushed_authorization_request_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app, "002").await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, "002").await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                Default::default(),
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let _request_uri = par(&mut app, issuer_state).await;
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_interactive_authorization_request_flow() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app, "002").await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, "002").await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                Default::default(),
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let _interactive_authorization_request = interactive_authorization_request(&mut app, issuer_state).await;
    }
}
