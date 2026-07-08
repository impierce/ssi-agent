use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::IntoResponse,
    routing::get,
    Router,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use std::{
    future::ready,
    sync::OnceLock,
    time::Instant,
};

static RECORDER_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Returns the global Prometheus recorder handle, installing the recorder on first use.
///
/// Call this early during startup so that metrics recorded before the `/metrics` server is up (such as gauges
/// seeded from persisted state) are captured by the recorder instead of being dropped.
pub fn recorder_handle() -> PrometheusHandle {
    RECORDER_HANDLE.get_or_init(setup_metrics_recorder).clone()
}

/// Source: https://github.com/tokio-rs/axum/blob/9ec85d69703a9065a1098bb43bd93113695d5ade/examples/prometheus-metrics/src/main.rs
pub fn metrics() -> Router {
    let recorder_handle = recorder_handle();
    Router::new().route("/metrics", get(move || ready(recorder_handle.render())))
}

fn setup_metrics_recorder() -> PrometheusHandle {
    const EXPONENTIAL_SECONDS: &[f64] = &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .unwrap()
        .install_recorder()
        .unwrap()
}

/// Source: https://github.com/tokio-rs/axum/blob/9ec85d69703a9065a1098bb43bd93113695d5ade/examples/prometheus-metrics/src/main.rs
pub async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [("method", method.to_string()), ("path", path), ("status", status)];

    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_requests_duration_seconds", &labels).record(latency);

    response
}
