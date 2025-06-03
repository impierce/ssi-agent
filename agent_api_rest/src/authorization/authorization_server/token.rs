use crate::authorization::AuthorizationState;
use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, PublicError};
use agent_authorization::application::token_issuance_service::TokenIssuanceService;
use agent_issuance::offer::command::OfferCommand;
use agent_issuance::state::IssuanceState;
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
    State((authorization_state, issuance_state)): State<(AuthorizationState, IssuanceState)>,
    Form(token_request): Form<TokenRequest>,
    // TODO: implement official oid4vci error response. This TODO is also in the `credential` endpoint.
) -> Result<Response, PublicError> {
    let token_response = TokenIssuanceService::issue_token(&authorization_state, token_request).expect("FIXME");

    // // Get the `pre_authorized_code` from the `TokenRequest`.
    // let pre_authorized_code = match &token_request {
    //     TokenRequest::PreAuthorizedCode {
    //         pre_authorized_code, ..
    //     } => pre_authorized_code,
    //     _ => return Err(PublicError::from(TokenErrorResponse::InvalidGrant)),
    // };

    // // Use the `pre_authorized_code` to get the `offer_id` from the `PreAuthorizedCodeView`.
    // let offer_id = query_handler(pre_authorized_code, &state.query.pre_authorized_code)
    //     .await?
    //     .ok_or_else(|| PublicError::from(TokenErrorResponse::InvalidGrant))?
    //     .offer_id;

    // let command = OfferCommand::CreateTokenResponse {
    //     offer_id: offer_id.clone(),
    //     token_request,
    // };

    // // Create a `TokenResponse` using the `offer_id` and `token_request`.
    // command_handler(&offer_id, &state.command.offer, command).await?;

    // // Use the `offer_id` to get the `token_response` from the `OfferView`.
    // query_handler(&offer_id, &state.query.offer)
    //     .await?
    //     .and_then(|offer_view| offer_view.token_response)
    //     .map(|token_response| (StatusCode::OK, Json(token_response)).into_response())
    //     .ok_or_else(internal_server_error)

    Ok((StatusCode::OK, Json(token_response)).into_response())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        authorization::{
            self,
            authorization_server::{authorize::tests::authorize, par::tests::par},
        },
        issuance::{self, credentials::tests::credentials, offers::tests::offers},
    };
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
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
        let issuance_state = issuance_state::<InMemory>(Service::default(), Default::default()).await;

        // Uncomment for Pre-Authorized Code flow
        // initialize(&issuance_state).await.unwrap();

        // let mut app = issuance::router(issuance_state.clone());

        // credentials(&mut app).await;
        // let _pre_authorized_code = offers(&mut app).await;

        let authorization_state = authorization_state::<InMemory>(Default::default()).await;
        let mut app = authorization::router((authorization_state, issuance_state));

        // let request_uri = par(&mut app).await;

        // let code = authorize(&mut app, request_uri).await;

        // let _access_token = token(&mut app, code).await;
    }
}
