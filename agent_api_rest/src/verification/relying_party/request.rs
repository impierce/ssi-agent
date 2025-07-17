use crate::handlers::query_handler;
use agent_verification::state::VerificationState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;

/// Instead of directly embedding the Authorization Request into a QR-code or deeplink, the `Relying Party` can embed a
/// `request_uri` that points to this endpoint from where the Authorization Request Object can be retrieved.
/// As described here: https://www.rfc-editor.org/rfc/rfc9101.html#name-passing-a-request-object-by-
#[axum_macros::debug_handler]
pub(crate) async fn request(
    State(verification_state): State<VerificationState>,
    Path(request_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&request_id, &verification_state.query.authorization_request)
        .await?
        .and_then(|authorization_request_view| authorization_request_view.signed_authorization_request_object)
        .map(|signed_authorization_request_object| {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/oauth-authz-req+jwt")],
                signed_authorization_request_object,
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    // use crate::verification::router;
    // // authorization_requests::tests::authorization_requests,
    // use agent_secret_manager::service::Service;
    // use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use tower::Service as _;

    pub async fn request(app: &mut Router, state: String) {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("/request/{state}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/oauth-authz-req+jwt"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: String = String::from_utf8(body.to_vec()).unwrap();

        let header = body.split_once('.').unwrap().0;
        assert_eq!(header, "eyJ0eXAiOiJvYXV0aC1hdXRoei1yZXErand0IiwiYWxnIjoiRWREU0EiLCJraWQiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCN6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QifQ");
    }

    // #[tokio::test]
    // #[tracing_test::traced_test]
    // async fn test_request_endpoint() {
    //     let verification_state = in_memory::verification_state(Service::default(), Default::default()).await;

    //     let mut app = router(verification_state);

    //     let form_url_encoded_authorization_request = authorization_requests(&mut app, false).await;

    //     // Extract the state from the form_url_encoded_authorization_request.
    //     let state = form_url_encoded_authorization_request
    //         .split("%2F")
    //         .last()
    //         .unwrap()
    //         .to_string();

    //     request(&mut app, state).await;
    // }
}
