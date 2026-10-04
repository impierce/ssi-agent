use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::IntoResponse,
};
use opentelemetry::{metrics::Histogram, KeyValue};
use std::{sync::OnceLock, time::Instant};

/// Explicit bucket boundaries for the request duration histogram, following the
/// [OpenTelemetry semantic conventions for HTTP metrics](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/).
const REQUEST_DURATION_SECONDS_BOUNDARIES: [f64; 11] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];

/// The `http.server.request.duration` histogram, created lazily so that it is bound to the meter provider that
/// is globally registered once the application is up (a no-op provider when OpenTelemetry is not enabled).
fn request_duration() -> &'static Histogram<f64> {
    static REQUEST_DURATION: OnceLock<Histogram<f64>> = OnceLock::new();

    REQUEST_DURATION.get_or_init(|| {
        opentelemetry::global::meter("unicore")
            .f64_histogram("http.server.request.duration")
            .with_unit("s")
            .with_description("Duration of HTTP server requests.")
            .with_boundaries(REQUEST_DURATION_SECONDS_BOUNDARIES.to_vec())
            .build()
    })
}

/// Middleware recording the [`http.server.request.duration`](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/#metric-httpserverrequestduration)
/// histogram for every request. The request count per route/method/status is derived from the histogram's count.
pub async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let route = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let attributes = [
        KeyValue::new("http.request.method", method.to_string()),
        KeyValue::new("http.route", route),
        KeyValue::new("http.response.status_code", i64::from(response.status().as_u16())),
    ];
    request_duration().record(start.elapsed().as_secs_f64(), &attributes);

    response
}
