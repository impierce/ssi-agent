use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, into_response, PublicError};
use agent_issuance::{offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use oid4vci::errors::TokenErrorResponse;
use oid4vci::token_request::TokenRequest;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State(state): State<IssuanceState>,
    Form(token_request): Form<TokenRequest>,
    // TODO: implement official oid4vci error response. This TODO is also in the `credential` endpoint.
) -> Response {
    // Get the `pre_authorized_code` from the `TokenRequest`.
    let pre_authorized_code = match &token_request {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => return into_response(PublicError::from(TokenErrorResponse::InvalidGrant)),
    };

    // Use the `pre_authorized_code` to get the `offer_id` from the `PreAuthorizedCodeView`.
    let offer_id = match query_handler(pre_authorized_code, &state.query.pre_authorized_code).await {
        Ok(Some(view)) => view.offer_id,
        Ok(None) => return into_response(PublicError::from(TokenErrorResponse::InvalidGrant)),
        Err(_) => return internal_server_error(),
    };

    let command = OfferCommand::CreateTokenResponse {
        offer_id: offer_id.clone(),
        token_request,
    };

    if let Err(_) = command_handler(&offer_id, &state.command.offer, command).await {
        return internal_server_error();
    }
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(offer_view)) => {
            if let Some(token_response) = offer_view.token_response {
                (StatusCode::OK, Json(token_response)).into_response()
            } else {
                internal_server_error()
            }
        }
        Ok(None) => internal_server_error(),
        Err(_) => internal_server_error(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        issuance::{credentials::tests::credentials, offers::tests::offers, router},
        tests::BASE_URL,
    };
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::token_response::TokenResponse;
    use tower::Service as _;

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

    #[serial_test::serial]
    #[tokio::test]
    async fn test_token_endpoint() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;
        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let pre_authorized_code: String = offers(&mut app).await.unwrap();

        let _access_token = token(&mut app, pre_authorized_code).await;
    }
}
