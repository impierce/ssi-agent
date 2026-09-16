use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use axum::{extract::State, http::StatusCode, response::IntoResponse};

#[derive(Clone, Default)]
pub struct ReadinessState {
    ready: Arc<AtomicBool>,
}

impl ReadinessState {
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn mark_not_ready(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

pub async fn readyz(State(readiness): State<ReadinessState>) -> impl IntoResponse {
    if readiness.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readiness_probe_reflects_shared_state() {
        let readiness = ReadinessState::default();
        let clone = readiness.clone();

        assert_eq!(
            readyz(State(clone.clone())).await.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        readiness.mark_ready();

        assert_eq!(readyz(State(clone)).await.into_response().status(), StatusCode::OK);

        readiness.mark_not_ready();

        assert_eq!(
            readyz(State(readiness)).await.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
