use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Tracks whether the application has finished its startup validation and is ready to serve
/// traffic.
///
/// This is a cheap-to-clone handle (an `Arc` internally): every clone observes and can mutate the
/// same underlying state. It starts out **not ready**; [`Readiness::set_ready`] and
/// [`Readiness::set_not_ready`] are expected to be called exactly once, right after the startup
/// event-replay validation pass completes (see `agent_application::run`).
#[derive(Clone)]
pub struct Readiness {
    ready: Arc<AtomicBool>,
    reason: Arc<RwLock<Option<String>>>,
}

impl Readiness {
    /// Creates a new handle, initially **not ready**.
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(RwLock::new(None)),
        }
    }

    /// Creates a new handle that is already marked ready. Useful for callers that don't perform
    /// (or don't care about) startup replay validation, e.g. ad hoc router construction in tests.
    pub fn new_ready() -> Self {
        let readiness = Self::new();
        readiness.set_ready();
        readiness
    }

    /// Marks the application as ready to serve traffic.
    pub fn set_ready(&self) {
        *self.reason.write().unwrap() = None;
        self.ready.store(true, Ordering::SeqCst);
    }

    /// Marks the application as not ready, recording `reason` for the `/readyz` response body.
    pub fn set_not_ready(&self, reason: impl Into<String>) {
        *self.reason.write().unwrap() = Some(reason.into());
        self.ready.store(false, Ordering::SeqCst);
    }

    /// Returns whether the application currently considers itself ready.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

impl Default for Readiness {
    fn default() -> Self {
        Self::new()
    }
}

/// A readiness probe following application monitoring conventions.
///
/// Returns `200 OK` once startup validation has completed successfully, `503 Service Unavailable`
/// otherwise (including a `reason` describing the failure). Unlike `/healthz`, this endpoint may
/// legitimately report failure while the process keeps running: that's the point — it lets an
/// orchestrator hold back traffic (or an old revision) without the process being killed.
pub async fn readyz(State(readiness): State<Readiness>) -> impl IntoResponse {
    if readiness.is_ready() {
        (StatusCode::OK, Json(json!({ "status": "ready" }))).into_response()
    } else {
        let reason = readiness.reason.read().unwrap().clone().unwrap_or_default();
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "not_ready", "reason": reason })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn router(readiness: Readiness) -> axum::Router {
        axum::Router::new()
            .route("/readyz", axum::routing::get(readyz))
            .with_state(readiness)
    }

    #[tokio::test]
    async fn readyz_flips_from_503_to_200() {
        let readiness = Readiness::new();
        let app = router(readiness.clone());

        // Initially not ready: 503 with a `not_ready` status.
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "not_ready");

        // Flip to ready: 200 with a `ready` status.
        readiness.set_ready();

        let response = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ready");
    }

    #[tokio::test]
    async fn readyz_reports_the_failure_reason() {
        let readiness = Readiness::new();
        readiness.set_not_ready("event replay validation failed for aggregate type `Offer`");
        let app = router(readiness);

        let response = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "not_ready");
        assert_eq!(
            json["reason"],
            "event replay validation failed for aggregate type `Offer`"
        );
    }
}
