use std::convert::Infallible;

use crate::error::type_url;
use axum::{
    extract::{rejection::JsonRejection, FromRequest, FromRequestParts, Query, Request},
    Json,
};
use http::StatusCode;
use http_api_problem::ApiError;
use serde::de::DeserializeOwned;
use shared_kernel::authorization::Actor;

/// Optional actor extracted from request extensions.
///
/// The actor extraction middleware stores an `Actor` in request extensions when
/// one is available. Handlers can use this extractor to receive that value as an
/// `Option<Actor>` without depending on Axum's `Extension` wrapper directly.
pub struct RequestActor(pub Option<Actor>);

impl<S> FromRequestParts<S> for RequestActor
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Actor>().cloned()))
    }
}

/// Query parameters of a UniCore API operation.
///
/// Wraps [`axum::extract::Query`] so that a query string that does not fit the parameters is rejected with an RFC 9457
/// problem (`400`, `application/problem+json`) instead of plain text.
pub struct QueryParams<T>(pub T);

impl<T, S> FromRequestParts<S> for QueryParams<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut http::request::Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(params)| QueryParams(params))
            .map_err(|rejection| {
                ApiError::builder(StatusCode::BAD_REQUEST)
                    .title("Invalid Query Parameters")
                    .type_url(type_url("request#invalid-query"))
                    .message(rejection.body_text())
                    .finish()
            })
    }
}

/// JSON request body of a UniCore API operation.
///
/// Wraps [`axum::Json`] so that every rejection is an RFC 9457 problem (`application/problem+json`)
/// instead of plain text, and requires the body to be a JSON object: `serde` would otherwise
/// deserialize a struct from a JSON array of its fields in order (`[null, null]`).
///
/// Status codes follow [`axum::Json`]: `415` without a JSON `Content-Type`, `400` for a body that is
/// not valid JSON, `422` for valid JSON of the wrong shape.
pub struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<serde_json::Value>::from_request(request, state)
            .await
            .map_err(json_rejection)?;

        if !value.is_object() {
            return Err(invalid_body("The request body must be a JSON object."));
        }

        serde_json::from_value(value)
            .map(JsonBody)
            .map_err(|error| invalid_body(error.to_string()))
    }
}

fn json_rejection(rejection: JsonRejection) -> ApiError {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => ApiError::builder(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .title("Unsupported Media Type")
            .type_url(type_url("request#unsupported-media-type"))
            .message("The request body must be sent with `Content-Type: application/json`.")
            .finish(),
        JsonRejection::JsonDataError(error) => invalid_body(error.body_text()),
        rejection => ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Malformed Request Body")
            .type_url(type_url("request#malformed-body"))
            .message(rejection.body_text())
            .finish(),
    }
}

fn invalid_body(message: impl Into<String>) -> ApiError {
    ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
        .title("Invalid Request Body")
        .type_url(type_url("request#invalid-body"))
        .message(message.into())
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::post, Router};
    use serde::Deserialize;
    use serde_json::Value;
    use tower::ServiceExt as _;

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Body2 {
        first: Option<String>,
        second: Option<String>,
    }

    async fn send(content_type: Option<&str>, body: &str) -> (StatusCode, Option<Value>) {
        let app = Router::new().route(
            "/",
            post(|JsonBody(_): JsonBody<Body2>| async { StatusCode::NO_CONTENT }),
        );
        let mut request = Request::builder().method("POST").uri("/");
        if let Some(content_type) = content_type {
            request = request.header(http::header::CONTENT_TYPE, content_type);
        }
        let response = app
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .map(|value| value.to_str().unwrap().to_string());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        if status != StatusCode::NO_CONTENT {
            assert_eq!(content_type.as_deref(), Some("application/problem+json"));
        }
        (status, serde_json::from_slice(&bytes).ok())
    }

    #[tokio::test]
    async fn accepts_a_json_object() {
        let (status, _) = send(Some("application/json"), r#"{"first": "a"}"#).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn rejections_are_problem_details() {
        for (content_type, body, expected_status, expected_type) in [
            (
                None,
                r#"{"first": "a"}"#,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "request#unsupported-media-type",
            ),
            (
                Some("text/plain"),
                r#"{"first": "a"}"#,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "request#unsupported-media-type",
            ),
            (
                Some("application/json"),
                r#"{"first": "a""#,
                StatusCode::BAD_REQUEST,
                "request#malformed-body",
            ),
            (
                Some("application/json"),
                r#"{"first": 1}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
                "request#invalid-body",
            ),
            // `serde` reads a struct from an array of its fields, which no client means to send.
            (
                Some("application/json"),
                "[null, null]",
                StatusCode::UNPROCESSABLE_ENTITY,
                "request#invalid-body",
            ),
            (
                Some("application/json"),
                "[]",
                StatusCode::UNPROCESSABLE_ENTITY,
                "request#invalid-body",
            ),
            (
                Some("application/json"),
                "null",
                StatusCode::UNPROCESSABLE_ENTITY,
                "request#invalid-body",
            ),
        ] {
            let (status, problem) = send(content_type, body).await;
            assert_eq!(status, expected_status, "{body}");
            assert_eq!(problem.unwrap()["type"], type_url(expected_type), "{body}");
        }
    }
}
