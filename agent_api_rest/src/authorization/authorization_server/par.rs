use crate::{issuance::error::PublicError, utils::StringifiedForm};
use agent_authorization::application::pushed_authorization_service::PushedAuthorizationService;
use agent_authorization::state::AuthorizationState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use oid4vci::authorization_request::AuthorizationRequest;

#[axum_macros::debug_handler]
pub(crate) async fn par(
    State(state): State<AuthorizationState>,
    StringifiedForm(pushed_authorization_request): StringifiedForm<AuthorizationRequest>,
) -> Result<Response, PublicError> {
    let pushed_authorization_response =
        PushedAuthorizationService::handle_pushed_authorization_request(&state, pushed_authorization_request)
            .await
            // TODO: implement proper error handling
            .map_err(|_err| PublicError::InternalServerError)?;

    Ok((StatusCode::CREATED, Json(pushed_authorization_response)).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        authorization,
        issuance::{self, credentials::tests::credentials, offers::tests::offers},
    };
    use agent_authorization::state::UNIME_CLIENT_ID;
    use agent_authorization::{
        domain::oauth2_authorization_request::aggregate::test_utils::code_challenge, state::UNIME_REDIRECT_URI,
    };
    use agent_secret_manager::service::Service;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::utils::form_urlencoded::to_form_urlencoded_string;
    use oid4vci::{
        authorization_details::{AuthorizationDetailsObject, CredentialConfigurationOrFormat, OpenidCredential},
        authorization_request::CodeChallengeMethod,
        credential_format_profiles::CredentialFormats,
        credential_offer::AuthorizationCode,
        wallet::PushedAuthorizationResponse,
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
                            authorization_details: vec![AuthorizationDetailsObject {
                                r#type: OpenidCredential::Type,
                                locations: None,
                                credential_configuration_or_format: CredentialConfigurationOrFormat::<
                                    CredentialFormats,
                                >::CredentialConfigurationId {
                                    credential_configuration_id: "configuration_id".to_string(),
                                    parameters: None,
                                },
                                claims: None,
                            }],
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

    #[serial_test::serial]
    #[tokio::test]
    async fn test_pushed_authorization_request_endpoint() {
        let issuance_state = issuance_state(&InMemory, Service::default(), Default::default()).await;

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app).await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, "002").await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = authorization_state(&InMemory, Service::default(), Default::default()).await;
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let _request_uri = par(&mut app, issuer_state).await;
    }
}
