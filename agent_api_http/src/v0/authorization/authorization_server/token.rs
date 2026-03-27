use crate::v0::authorization::AuthorizationState;
use crate::v0::issuance::error::PublicError;
use agent_authorization::application::token_issuance_service::TokenIssuanceService;
use agent_issuance::state::IssuanceState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use oid4vci::token_request::TokenRequest;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State((authorization_state, issuance_state)): State<(Arc<AuthorizationState>, Arc<IssuanceState>)>,
    Form(token_request): Form<TokenRequest>,
) -> Result<Response, PublicError> {
    let token_response =
        TokenIssuanceService::issue_token(&authorization_state, &issuance_state, token_request).await?;

    Ok((StatusCode::OK, Json(token_response)).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::v0::{
        authorization::{
            self,
            authorization_server::{
                authorize::tests::{authorize_after_consent, authorize_before_consent},
                consent::tests::{get_consent, post_consent},
                par::tests::par,
            },
        },
        issuance::{self, credentials::tests::credentials, offers::tests::offers},
    };
    use agent_authorization::services::AuthorizationServices;
    use agent_authorization::state::UNIME_CLIENT_ID;
    use agent_authorization::{
        domain::oauth2_authorization_request::aggregate::test_utils::code_verifier, state::UNIME_REDIRECT_URI,
    };
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
        credential_offer::{AuthorizationCode, PreAuthorizedCode},
        token_response::TokenResponse,
    };
    use rstest::rstest;
    use tower::Service as _;

    pub async fn token(
        app: &mut Router,
        is_pre_authorized: bool,
        (authorization_code, pre_authorized_code): (Option<AuthorizationCode>, Option<PreAuthorizedCode>),
    ) -> String {
        let code = if let Some(PreAuthorizedCode {
            pre_authorized_code, ..
            // TODO: handle `tx_code`
        }) = pre_authorized_code
        {
            (!is_pre_authorized).then(|| {
                panic!("Expected authorization code, but got pre-authorized code");
            });

            pre_authorized_code
        } else if let Some(AuthorizationCode { issuer_state, .. }) = authorization_code {
            is_pre_authorized.then(|| {
                panic!("Expected pre-authorized code, but got authorization code");
            });

            let issuer_state = issuer_state.unwrap();

            let request_uri = par(app, issuer_state).await;

            let see_other_location =
                authorize_before_consent(app, UNIME_CLIENT_ID.to_string(), request_uri.clone()).await;

            get_consent(app, see_other_location.clone()).await;

            let see_other_location = post_consent(app, UNIME_CLIENT_ID.to_string(), request_uri, true).await;

            let code = authorize_after_consent(app, see_other_location).await;

            code
        } else {
            panic!("Expected either authorization code or pre-authorized code, but got neither");
        };

        let token_request = if is_pre_authorized {
            TokenRequest::PreAuthorizedCode {
                pre_authorized_code: code,
                tx_code: None,
                authorization_details: None,
            }
        } else {
            let code_verifier = String::from_utf8(code_verifier().to_vec()).unwrap();

            TokenRequest::AuthorizationCode {
                client_id: UNIME_CLIENT_ID.to_string(),
                code,
                code_verifier: Some(code_verifier),
                redirect_uri: UNIME_REDIRECT_URI.parse().ok(),
                authorization_details: None,
            }
        };

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/token")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(to_form_urlencoded_string(&token_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let token_response: TokenResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(token_response.token_type, "bearer");
        token_response.access_token
    }

    #[rstest]
    #[case::pre_authorized_code(true)]
    #[case::authorization_code(false)]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_token_endpoint(#[case] is_pre_authorized: bool) {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        let credential_configuration_id = if is_pre_authorized {
            "001".to_string()
        } else {
            "002".to_string()
        };

        credentials(&mut app, &credential_configuration_id).await;
        let grants = offers(&mut app, &credential_configuration_id).await.unwrap();

        let authorization_state =
            Arc::new(authorization_state(&InMemory, AuthorizationServices::default().await, Default::default()).await);

        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state.clone(), issuance_state.clone()));

        let _access_token = token(&mut app, is_pre_authorized, grants).await;
    }
}
