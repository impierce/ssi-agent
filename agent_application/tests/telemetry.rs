//! End-to-end test of the OpenTelemetry export against `grafana/otel-lgtm` running in Docker (`docker-tests`
//! feature): the application's traces, logs and metrics must arrive in Tempo, Loki and Prometheus.
//!
//! Traces and metrics are exported via OTLP/gRPC, logs via OTLP/HTTP, to cover both supported protocols.

use agent_application::{router, state, telemetry::init_telemetry};
use agent_secret_manager::subject::Subject;
use agent_shared::config::LogFormat;
use axum::{body::Body, extract::Request, http::StatusCode};
use serde_json::Value;
use std::{future::Future, sync::Arc, time::Duration};
use testcontainers_modules::testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};
use tower::ServiceExt;

/// Polls `check` until it yields a value, since the backends ingest exported telemetry asynchronously.
async fn eventually<T, F: Future<Output = Option<T>>>(what: &str, mut check: impl FnMut() -> F) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} did not arrive within 60 seconds"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn get_json(client: &reqwest::Client, url: &str, query: &[(&str, &str)]) -> Option<Value> {
    client.get(url).query(query).send().await.ok()?.json().await.ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn traces_logs_and_metrics_are_exported_via_otlp() {
    let container = GenericImage::new("grafana/otel-lgtm", "0.34.0")
        .with_exposed_port(4317.tcp())
        .with_exposed_port(4318.tcp())
        .with_exposed_port(3100.tcp())
        .with_exposed_port(3200.tcp())
        .with_exposed_port(9090.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "The OpenTelemetry collector and the Grafana LGTM stack are up and running",
        ))
        .with_startup_timeout(Duration::from_secs(180))
        .start()
        .await
        .unwrap();
    let host = container.get_host().await.unwrap();
    let url = |port: u16| {
        let container = &container;
        let host = host.clone();
        async move { format!("http://{host}:{}", container.get_host_port_ipv4(port).await.unwrap()) }
    };
    let (otlp_grpc, otlp_http, loki, tempo, prometheus) = (
        url(4317).await,
        url(4318).await,
        url(3100).await,
        url(3200).await,
        url(9090).await,
    );

    // Each file in `tests/` runs in its own process, so this only affects this test binary.
    let service_name = format!("unicore-telemetry-test-{}", std::process::id());
    for variable in [
        "OTEL_SDK_DISABLED",
        "OTEL_TRACES_EXPORTER",
        "OTEL_LOGS_EXPORTER",
        "OTEL_METRICS_EXPORTER",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
    ] {
        std::env::remove_var(variable);
    }
    std::env::set_var("UNICORE__PROFILE", "development");
    std::env::set_var("UNICORE__EVENT_STORE__TYPE", "in_memory");
    std::env::set_var("OTEL_SERVICE_NAME", &service_name);
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &otlp_grpc);
    std::env::set_var("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL", "http/protobuf");
    std::env::set_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", format!("{otlp_http}/v1/logs"));

    let guard = init_telemetry(&LogFormat::Json);

    let app = router(state(Arc::new(Subject::test_subject().await)).await.unwrap());
    for uri in ["/v0/profile", "/healthz"] {
        let response = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
    }

    // Dropping the guard shuts the providers down, which flushes all pending exports. It blocks, so it runs on a
    // blocking thread to keep the runtime available for the gRPC exporters.
    tokio::task::spawn_blocking(move || drop(guard)).await.unwrap();

    let client = reqwest::Client::new();

    let traces_query = format!("service.name={service_name}");
    eventually("traces", || async {
        get_json(&client, &format!("{tempo}/api/search"), &[("tags", &traces_query)])
            .await
            .filter(|traces| traces["traces"].as_array().is_some_and(|traces| !traces.is_empty()))
    })
    .await;

    let logs_query = format!("{{service_name=\"{service_name}\"}}");
    eventually("logs", || async {
        get_json(
            &client,
            &format!("{loki}/loki/api/v1/query_range"),
            &[("query", &logs_query), ("since", "1h")],
        )
        .await
        .filter(|logs| {
            logs["data"]["result"]
                .as_array()
                .is_some_and(|result| !result.is_empty())
        })
    })
    .await;

    let metrics_query = format!("http_server_request_duration_seconds_count{{job=\"{service_name}\"}}");
    let metrics = eventually("metrics", || async {
        get_json(
            &client,
            &format!("{prometheus}/api/v1/query"),
            &[("query", &metrics_query)],
        )
        .await
        .filter(|metrics| {
            metrics["data"]["result"]
                .as_array()
                .is_some_and(|result| !result.is_empty())
        })
    })
    .await;
    let routes: Vec<&str> = metrics["data"]["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|series| series["metric"]["http_route"].as_str())
        .collect();
    assert!(routes.contains(&"/v0/profile"), "{metrics}");
}
