use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, PublicError};
use agent_authorization::application::pushed_authorization_service::{
    PushedAuthorizationRequest, PushedAuthorizationService,
};
use agent_authorization::state::AuthorizationState;
use agent_issuance::{offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};

#[axum_macros::debug_handler]
pub(crate) async fn par(
    State(state): State<AuthorizationState>,
    Form(pushed_authorization_request): Form<PushedAuthorizationRequest>,
) -> Result<Response, PublicError> {
    let pushed_authorization_response =
        PushedAuthorizationService::handle_pushed_authorization_request(&state, pushed_authorization_request)
            .await
            .expect("FIXME");

    Ok((StatusCode::CREATED, Json(pushed_authorization_response)).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        authorization,
        issuance::{self, credentials::tests::credentials, offers::tests::offers},
    };
    use agent_authorization::application::pushed_authorization_service::PushedAuthorizationResponse;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_offer::AuthorizationCode;
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
                        serde_urlencoded::to_string(&PushedAuthorizationRequest {
                            response_type: "code".to_string(),
                            state: "test_state".to_string(),
                            client_id: "test_client_id".to_string(),
                            redirect_uri: "unime://callback".parse().unwrap(),
                            code_challenge: Some("test_code_challenge".to_string()),
                            code_challenge_method: Some("S256".to_string()),
                            scope: "openid profile".to_string(),
                            client_assertion_type: None,
                            client_assertion: None,
                            issuer_state: Some(issuer_state),
                            // authorization_details: AuthorizationDetailsObject {
                            //     r#type: OpenidCredential::Type,
                            //     locations: None,
                            //     credential_configuration_or_format:
                            //         CredentialConfigurationOrFormat::CredentialConfigurationId {
                            //             credential_configuration_id: "configuration_id-FIXME".to_string(),
                            //             parameters: None,
                            //         },
                            // },
                        })
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
        assert_eq!(pushed_authorization_response.request_uri, uuid::Uuid::from_u128(0_u128));
        assert_eq!(pushed_authorization_response.expires_in, 3600);

        pushed_authorization_response.request_uri.urn().to_string()
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_pushed_authorization_request_endpoint() {
        let issuance_state = issuance_state::<InMemory>(Service::default(), Default::default()).await;

        initialize(&issuance_state).await.unwrap();

        let mut app = issuance::router(issuance_state.clone());

        credentials(&mut app).await;
        let (AuthorizationCode { issuer_state, .. }, _pre_authorized_code) = offers(&mut app).await.unwrap();
        let issuer_state = issuer_state.unwrap();

        println!("Issuer State: {}", issuer_state);

        let authorization_state = authorization_state::<InMemory>(Default::default()).await;
        let mut app = authorization::router((authorization_state, issuance_state));

        let _request_uri = par(&mut app, issuer_state).await;

        // FIXME
        println!(" Request URI: {}", _request_uri);
    }
}
