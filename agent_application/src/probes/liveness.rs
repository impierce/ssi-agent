use axum::http::StatusCode;
use axum::response::IntoResponse;

/// A simple liveness probe following application monitoring conventions.
#[utoipa::path(
    get,
    path = "/healthz",
    operation_id = "healthz",
    tags = ["Probes"],
    responses(
        (status = 200, description = "Application is alive"),
    )
)]
pub async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}
