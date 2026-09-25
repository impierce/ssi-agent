use axum::http::StatusCode;
use axum::response::IntoResponse;

/// A simple liveness probe following application monitoring conventions.
#[utoipa::path(
    get,
    path = "/livez",
    operation_id = "livez",
    tags = ["Probes"],
    responses(
        (status = 200, description = "Application is alive"),
    )
)]
pub async fn livez() -> impl IntoResponse {
    StatusCode::OK
}

/// An alias for the liveness probe.
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
    livez().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn legacy_health_endpoint_matches_liveness_endpoint() {
        assert_eq!(
            healthz().await.into_response().status(),
            livez().await.into_response().status()
        );
    }
}
