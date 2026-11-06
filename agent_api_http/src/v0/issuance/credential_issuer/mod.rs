pub mod credential;
pub mod credential_offer;
pub mod notification;
pub mod token_status_list;
pub mod well_known;

use crate::v0::issuance::error::PublicError;
use axum::extract::{rejection::JsonRejection, Json};
use axum_auth::AuthBearer;

/// Unwraps the bearer token and JSON body of a protocol request. Their rejections are reported in the OID4VC error
/// format these endpoints document, with the endpoint's own invalid-request error code, rather than as the extractors'
/// plain-text responses.
pub(crate) fn protocol_request<T>(
    bearer: Result<AuthBearer, axum_auth::Rejection>,
    body: Result<Json<T>, JsonRejection>,
    invalid_request: impl Fn() -> PublicError,
) -> Result<(String, T), PublicError> {
    let AuthBearer(access_token) = bearer.map_err(|_| invalid_request())?;
    let Json(body) = body.map_err(|rejection| match rejection {
        JsonRejection::MissingJsonContentType(_) => PublicError::UnsupportedMediaType,
        _ => invalid_request(),
    })?;
    Ok((access_token, body))
}

#[cfg(test)]
mod tests {
    use crate::v0::issuance::{credentials::tests::setup_library_state, router};
    use agent_issuance::{issuance_state, services::IssuanceServices};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use axum::{
        body::Body,
        http::{header, Method, Request, StatusCode},
    };
    use rstest::rstest;
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tower::ServiceExt;

    const VALID_BODY: &str =
        r#"{"notification_id":"n","event":"credential_accepted","credential_configuration_id":"c"}"#;

    #[rstest]
    #[case::credential_without_authorization("/openid4vci/credential", None, VALID_BODY, "invalid_credential_request")]
    #[case::credential_with_basic_authorization(
        "/openid4vci/credential",
        Some("Basic dXNlcjpwYXNz"),
        VALID_BODY,
        "invalid_credential_request"
    )]
    #[case::credential_with_malformed_body(
        "/openid4vci/credential",
        Some("Bearer t"),
        "{",
        "invalid_credential_request"
    )]
    #[case::credential_with_invalid_body(
        "/openid4vci/credential",
        Some("Bearer t"),
        r#"{"proofs":[]}"#,
        "invalid_credential_request"
    )]
    #[case::notification_without_authorization(
        "/openid4vci/notification",
        None,
        VALID_BODY,
        "invalid_notification_request"
    )]
    #[case::notification_with_malformed_body(
        "/openid4vci/notification",
        Some("Bearer t"),
        "{",
        "invalid_notification_request"
    )]
    #[serial_test::serial]
    #[tokio::test]
    async fn rejected_protocol_requests_use_the_oid4vc_error_format(
        #[case] uri: &str,
        #[case] authorization: Option<&str>,
        #[case] body: &'static str,
        #[case] error: &str,
    ) {
        let response = send(uri, authorization, "application/json", body).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "error": error })
        );
    }

    #[rstest]
    #[case::credential("/openid4vci/credential")]
    #[case::notification("/openid4vci/notification")]
    #[serial_test::serial]
    #[tokio::test]
    async fn protocol_requests_without_json_content_type_are_unsupported(#[case] uri: &str) {
        let response = send(uri, Some("Bearer t"), "text/plain", VALID_BODY).await;

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    async fn send(
        uri: &str,
        authorization: Option<&str>,
        content_type: &str,
        body: &'static str,
    ) -> axum::response::Response {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
        let library_state = setup_library_state(&issuance_state).await;
        let app = router((issuance_state, library_state));

        let mut request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type);
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }

        app.oneshot(request.body(Body::from(body)).unwrap()).await.unwrap()
    }
}
