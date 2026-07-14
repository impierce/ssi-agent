# Metrics

UniCore publishes metrics through **two independent pipelines**:

| Pipeline | Transport | Activation | Consumers |
| --- | --- | --- | --- |
| **Prometheus** | Pull: scrape the `/metrics` endpoint (separate port, default `9090`) | `metrics.enabled` configuration (default: `true`) | Prometheus, Grafana Agent, any scraper |
| **OpenTelemetry** | Push: OTLP/gRPC to a collector | Presence of an `OTEL_EXPORTER_OTLP_*` endpoint environment variable | OTel Collector, Grafana, Datadog, ... |

The two pipelines are wired independently — a metric only shows up in both if it is recorded to both (see [Adding a new metric](#adding-a-new-metric)).

## The Prometheus `/metrics` endpoint

- Served on its **own port** (default `9090`), configured via the `metrics` section (`UNICORE__METRICS__ENABLED`, `UNICORE__METRICS__PORT`). It is enabled by default and **not affected by any `OTEL_*` environment variable**.
- Backed by the [`metrics`](https://docs.rs/metrics) crate facade with the [`metrics-exporter-prometheus`](https://docs.rs/metrics-exporter-prometheus) recorder. The recorder is a process-wide singleton installed once at startup (`agent_api_http::metrics::recorder_handle()`), *before* the application state is built, so values recorded during startup (e.g. gauges seeded from persisted state) are not dropped.
- Everything recorded through the `metrics::` macros anywhere in the workspace ends up here automatically.

Currently published metrics:

| Metric | Type | Source |
| --- | --- | --- |
| `http_requests_total{method,path,status}` | counter | `agent_api_http::metrics::track_metrics` middleware |
| `http_requests_duration_seconds{method,path,status}` | histogram | `agent_api_http::metrics::track_metrics` middleware |
| `credentials_count` | gauge | `agent_application::credential_metrics::CredentialCountProjection` |

## The OpenTelemetry pipeline

OpenTelemetry export (traces, logs **and metrics**) is activated purely by the standard
[OTLP exporter environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/) — there is no UniCore configuration key for it:

```sh
# Activates export of all three signals via OTLP/gRPC:
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
# Optional; defaults to "unicore":
OTEL_SERVICE_NAME="unicore"
# Disable individual signals when the collector does not support them
# (e.g. Jaeger only accepts traces):
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
# Force-disable everything regardless of the endpoint variables:
OTEL_SDK_DISABLED=true
```

If **none** of the `OTEL_EXPORTER_OTLP_*` endpoint variables is set, no OpenTelemetry
provider is installed: the global meter provider stays a no-op, nothing is buffered or
exported, and console logging behaves exactly as before. The Prometheus `/metrics`
endpoint keeps working regardless.

The wiring lives in `agent_application/src/telemetry.rs` (`init_telemetry`), which is
called first thing in `agent_application::run()`. When active, metrics recorded through
the **global OpenTelemetry meter provider** are pushed periodically to the collector by
the `SdkMeterProvider`.

## Adding a new metric

There are two recording APIs, one per pipeline. Decide where the metric should be
visible and record accordingly (or to both, as `CredentialCountProjection` does).

### 1. Publish on `/metrics` (Prometheus)

Use the [`metrics`](https://docs.rs/metrics) macros anywhere in the code — no
registration step is needed, the first record creates the metric:

```rust
metrics::counter!("offers_created_total").increment(1);
metrics::gauge!("credentials_count").set(count as f64);
metrics::histogram!("signing_duration_seconds").record(elapsed);
```

Notes:

- Labels are passed as `&[("key", value)]`, see `track_metrics` in
  `agent_api_http/src/metrics.rs` for an example.
- Histograms are rendered as Prometheus summaries unless explicit buckets are configured
  for the metric name in `setup_metrics_recorder()`
  (`agent_api_http/src/metrics.rs`) — add a `set_buckets_for_metric` matcher there if
  your metric needs proper histogram buckets.
- Metrics recorded before the recorder is installed are silently dropped. The recorder
  is installed early in `run()`; if you add a new binary or entrypoint, call
  `agent_api_http::metrics::recorder_handle()` before recording anything.

### 2. Publish via OpenTelemetry (OTLP)

Create an instrument from the **global** meter provider and record to it:

```rust
opentelemetry::global::meter("unicore")
    .u64_gauge("credentials_count")
    .with_description("The number of credentials, excluding those reported as deleted by the holder.")
    .build()
    .record(count, &[]);
```

Notes:

- Always resolve the meter through `opentelemetry::global` — when OpenTelemetry is not
  activated this is a no-op provider and recording is essentially free.
- The instrument name defines the metric name at the collector; keep it identical to the
  Prometheus name so dashboards can correlate the two pipelines.
- Recording is synchronous and in-memory; the periodic exporter pushes the current state
  to the collector in the background.

### 3. Metrics derived from domain events (projections)

For metrics that are computed from the event stream (like `credentials_count`),
implement a [`cqrs_es::Query`](https://docs.rs/cqrs-es) for the aggregate and record the
metric in `dispatch`. Use `CredentialCountProjection`
(`agent_application/src/credential_metrics.rs`) as the blueprint:

1. **Implement `Query<Aggregate>`**: fold the incoming `EventEnvelope`s into whatever
   state the metric needs, then record the new value to one or both pipelines.
2. **Attach the projection** to the aggregate when the state is built. For the
   `Credential` aggregate this is
   `agent_store::issuance_state_with_credential_queries(...)`, called in
   `agent_application/src/lib.rs` — note it must be attached in **all three**
   `EventStoreType` match arms (Postgres, MongoDB, InMemory).
3. **Seed from persisted state**: a projection only sees events dispatched while the
   process is running. If the metric must reflect pre-existing data, load a persisted
   view (e.g. the `all_credentials` list view) after the state is built and initialize
   the metric from it — see `CredentialCountProjection::seed`.

## Verifying locally

```sh
# Prometheus endpoint (no env vars needed; enabled by default on port 9090):
curl -s localhost:9090/metrics

# OTLP export against a local collector/Jaeger:
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317" cargo run
```

For a full local OTel stack covering all three signals (Tempo, Loki, Prometheus behind one
OTLP endpoint, with a Grafana UI), see [`dev/telemetry/compose.yaml`](../../dev/telemetry/compose.yaml).
