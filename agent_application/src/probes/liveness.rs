pub async fn healthz_handler() -> impl axum::response::IntoResponse {
    axum::http::StatusCode::OK
}
