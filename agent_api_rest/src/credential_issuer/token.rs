use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::command::OfferCommand,
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
    let pre_authorized_code = match token_request.clone() {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    let offer_id = state
        .offer
        .load_pre_authorized_code(&pre_authorized_code)
        .await
        .unwrap()
        .unwrap()
        .offer_id;

    let command = OfferCommand::CreateTokenResponse { token_request };

    match command_handler(offer_id.clone(), &state.offer, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler(offer_id, &state.offer).await {
        Ok(Some(view)) => (StatusCode::OK, Json(view.token_response.unwrap())).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
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
