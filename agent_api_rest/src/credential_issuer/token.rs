use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::{
        command::OfferCommand,
        queries::{OfferView, PreAuthorizedCodeView},
    },
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    Form,
};
use oid4vci::token_request::TokenRequest;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State(state): State<ApplicationState>,
    Form(token_request): Form<TokenRequest>,
) -> impl IntoResponse {
    // Get the `pre_authorized_code` from the `TokenRequest`.
    let pre_authorized_code = match &token_request {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => panic!(),
    };

    // Use the `pre_authorized_code` to get the `offer_id` from the `PreAuthorizedCodeView`.
    let offer_id = match query_handler(pre_authorized_code, &state.query.pre_authorized_code).await {
        Ok(Some(PreAuthorizedCodeView { offer_id })) => offer_id,
        _ => panic!(),
    };

    // Create a `TokenResponse` using the `offer_id` and `token_request`.
    match command_handler(
        &offer_id,
        &state.command.offer,
        OfferCommand::CreateTokenResponse { token_request },
    )
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        _ => panic!(),
    };

    // Use the `offer_id` to get the `token_response` from the `OfferView`.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView {
            token_response: Some(token_response),
            ..
        })) => (StatusCode::OK, Json(token_response)).into_response(),
        _ => panic!(),
    }
}

#[cfg(test)]
pub mod tests {
    use crate::{app, credentials::tests::credentials, offers::tests::offers, tests::BASE_URL};

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_store::in_memory;
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

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let token_response: TokenResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(token_response.token_type, "bearer");
        assert!(token_response.c_nonce.is_some());

        token_response.access_token
    }

    #[tokio::test]
    async fn test_token_endpoint() {
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        let _response = credentials(&mut app).await.unwrap();
        let pre_authorized_code = offers(&mut app).await;

        let _access_token = token(&mut app, pre_authorized_code).await;

        dbg!(_access_token);
    }
}
