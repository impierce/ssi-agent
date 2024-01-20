use agent_issuance::{
    handlers::{command_handler, query_handler},
    offer::{aggregate::Offer, command::OfferCommand, queries::OfferView},
    state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    Form,
};
use oid4vci::token_request::TokenRequest;

// use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn token(
    State(state): State<ApplicationState<Offer, OfferView>>,
    Form(token_request): Form<TokenRequest>,
) -> impl IntoResponse {
    let pre_authorized_code = match token_request.clone() {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code, ..
        } => pre_authorized_code,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let command = OfferCommand::CreateTokenResponse { token_request };

    match command_handler("OFF-0123".to_string(), &state, command).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    };

    match query_handler("OFF_98123".to_string(), &state).await {
        Ok(Some(view)) => {
            // TODO: This is a non-idiomatic way of finding the subject by using the pre-authorized_code in the token_request. We should use a aggregate/query instead.
            // let subject = view
            //     .subjects
            //     .iter()
            //     .find(|subject| subject.pre_authorized_code == pre_authorized_code);
            // if let Some(subject) = subject {
            //     (StatusCode::OK, Json(subject.token_response.clone())).into_response()
            // } else {
            //     StatusCode::NOT_FOUND.into_response()
            // }
            StatusCode::NOT_IMPLEMENTED.into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app,
        tests::{create_credential_offer, create_unsigned_credential, BASE_URL, PRE_AUTHORIZED_CODE},
    };

    use super::*;
    use agent_issuance::{
        services::IssuanceServices,
        startup_commands::startup_commands_server_config,
        state::{initialize, CQRS},
    };
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_token_endpoint() {
        let state = in_memory::ApplicationState::new(vec![], IssuanceServices {}).await;

        initialize(state.clone(), startup_commands_server_config(BASE_URL.clone())).await;

        create_unsigned_credential(state.clone()).await;
        create_credential_offer(state.clone()).await;

        let app = app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("/auth/token"))
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(format!(
                        "grant_type=urn:ietf:params:oauth:grant-type:pre-authorized_code&pre-authorized_code={}",
                        PRE_AUTHORIZED_CODE
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["access_token"].as_str().is_some());
        assert_eq!(body["token_type"], "bearer");
        assert!(body["c_nonce"].as_str().is_some());
    }
}
