use std::str::FromStr;

use agent_shared::{
    generate_random_string,
    handlers::{command_handler, query_handler},
};
use agent_verification::{
    authorization_request::{command::AuthorizationRequestCommand, queries::AuthorizationRequestView},
    state::VerificationState,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hyper::header;
use oid4vc_core::{client_metadata::ClientMetadata, DidMethod, SubjectSyntaxType};
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn authorization_requests(
    State(verification_state): State<VerificationState>,
    nonce: String,
) -> Response {
    info!("Request Body: {}", nonce);

    let state = generate_random_string();

    let command = AuthorizationRequestCommand::CreateAuthorizationRequest {
        nonce,
        state: state.clone(),
        // TODO: all this is SERVER_CONFIG
        client_metadata: Box::new(ClientMetadata::default().with_subject_syntax_types_supported(vec![
            SubjectSyntaxType::Did(DidMethod::from_str("did:key").unwrap()),
        ])),
    };

    if command_handler(&state, &verification_state.command.authorization_request, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    if command_handler(
        &state,
        &verification_state.command.authorization_request,
        AuthorizationRequestCommand::SignAuthorizationRequestObject,
    )
    .await
    .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Return the credential.
    match query_handler(&state, &verification_state.query.authorization_request).await {
        Ok(Some(AuthorizationRequestView {
            form_url_encoded_authorization_request,
            ..
        })) => (
            StatusCode::CREATED,
            [(header::LOCATION, &format!("/siopv2/request/{state}"))],
            Json(form_url_encoded_authorization_request),
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::app;
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use tower::Service;

    pub async fn authorization_requests(app: &mut Router) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/v1/authorization_requests")
                    .header(http::header::CONTENT_TYPE, mime::TEXT.as_ref())
                    .body(Body::from("nonce".to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let get_request_endpoint = response
            .headers()
            .get(http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let state = get_request_endpoint.split('/').last().unwrap().to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let form_url_encoded_authorization_request: String = serde_json::from_slice(&body).unwrap();
        assert_eq!(form_url_encoded_authorization_request, format!("siopv2://idtoken?client_id=did%3Akey%3Az6MkiieyoLMSVsJAZv7Jje5wWSkDEymUgkyF8kbcrjZpX3qd&request_uri=https%3A%2F%2Fmy-domain.example.org%2Fsiopv2%2Frequest%2F{state}"));

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_request_endpoint)
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        form_url_encoded_authorization_request
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_authorization_requests_endpoint() {
        let issuance_state = in_memory::issuance_state().await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;

        let mut app = app((issuance_state, verification_state));

        authorization_requests(&mut app).await;
    }
}
