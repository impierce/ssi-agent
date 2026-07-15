use std::sync::atomic::{AtomicBool, Ordering};

use axum::{http::StatusCode, response::IntoResponse};

static READY: AtomicBool = AtomicBool::new(false);

pub fn mark_ready() {
    READY.store(true, Ordering::Release);
}

pub async fn readyz() -> impl IntoResponse {
    if READY.load(Ordering::Acquire) {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
