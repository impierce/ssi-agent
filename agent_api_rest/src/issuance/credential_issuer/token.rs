use agent_issuance::{
    offer::{
        command::OfferCommand,
        queries::{pre_authorized_code::PreAuthorizedCodeView, OfferView},
    },
    state::IssuanceState,
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use oid4vci::token_request::TokenRequest;
use serde_json::json;
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State(state): State<IssuanceState>,
    Form(token_request): Form<TokenRequest>,
    // TODO: implement official oid4vci error response. This TODO is also in the `credential` endpoint.
) -> Response {
    info!("Request Body: {}", json!(token_request));

    // Get the `pre_authorized_code` from the `TokenRequest`.
    let pre_authorized_code = match &token_request {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    // Use the `pre_authorized_code` to get the `offer_id` from the `PreAuthorizedCodeView`.
    let offer_id = match query_handler(pre_authorized_code, &state.query.pre_authorized_code).await {
        Ok(Some(PreAuthorizedCodeView { offer_id })) => offer_id,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let command = OfferCommand::CreateTokenResponse {
        offer_id: offer_id.clone(),
        token_request,
    };

    // Create a `TokenResponse` using the `offer_id` and `token_request`.
    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Use the `offer_id` to get the `token_response` from the `OfferView`.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView {
            token_response: Some(token_response),
            ..
        })) => (StatusCode::OK, Json(token_response)).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use crate::{
        app,
        issuance::{credentials::tests::credentials, offers::tests::offers},
        tests::BASE_URL,
    };

    use super::*;
    use agent_issuance::{
        services::test_utils::test_issuance_services, startup_commands::startup_commands, state::initialize,
    };
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::token_response::TokenResponse;
    use tower::Service;

    pub async fn token(app: &mut Router, pre_authorized_code: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/token")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(format!(
                        "grant_type=urn:ietf:params:oauth:grant-type:pre-authorized_code&pre-authorized_code={}",
                        pre_authorized_code
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let token_response: TokenResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(token_response.token_type, "bearer");
        assert!(token_response.c_nonce.is_some());

        token_response.access_token
    }

    #[tokio::test]
    async fn test_token_endpoint() {
        let issuance_state = in_memory::issuance_state(test_issuance_services(), Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = app((issuance_state, verification_state));

        credentials(&mut app).await;
        let pre_authorized_code = offers(&mut app).await;

        let _access_token = token(&mut app, pre_authorized_code).await;
    }
}
