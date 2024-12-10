use axum::http::StatusCode;
use axum::response::IntoResponse;

/// A simple liveness probe following application monitoring conventions.
pub async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}
