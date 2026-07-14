# Metrics

UniCore publishes all metrics via **OpenTelemetry** (OTLP push). There is no scrape endpoint (such as `/metrics` used by Prometheus).

## Activating the export

OpenTelemetry export (traces, logs **and metrics**) is activated purely by the standard
[OTLP exporter environment variables](https://opentelemetry.io/docs/specs/otel/protocol/exporter/). There are no specific `UNICORE__` configuration keys for it.

```sh
# Activates export of all three signals via OTLP (gRPC by default):
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
# Optional; defaults to "grpc", set to "http/protobuf" for OTLP over HTTP:
OTEL_EXPORTER_OTLP_PROTOCOL="grpc"
# Optional; defaults to "unicore":
OTEL_SERVICE_NAME="unicore"
# Disable individual signals when the collector does not support them
# (e.g. Jaeger only accepts traces):
OTEL_LOGS_EXPORTER=none
OTEL_METRICS_EXPORTER=none
# Force-disable everything regardless of the endpoint variables:
OTEL_SDK_DISABLED=true
```

If **none** of the `OTEL_EXPORTER_OTLP_*` endpoint variables is set, no OpenTelemetry provider is installed: the global meter provider stays a no-op, nothing is buffered or exported, and console logging behaves exactly as before.

The wiring lives in `agent_application/src/telemetry.rs` (`init_telemetry`), which is called first thing in `agent_application::run()`. When active, metrics recorded through the **global OpenTelemetry meter provider** are pushed periodically to the collector by the `SdkMeterProvider`.

## Published metrics

| Metric                                                                                                        | Type      | Source                                              |
| ------------------------------------------------------------------------------------------------------------- | --------- | --------------------------------------------------- |
| `http.server.request.duration` (attributes: `http.request.method`, `http.route`, `http.response.status_code`) | histogram | `agent_api_http::metrics::track_metrics` middleware |

The HTTP histogram follows the [OpenTelemetry semantic conventions](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/#metric-httpserverrequestduration); the request count per route/method/status is derived from the histogram's count.

## Cardinality: what makes a good metric

Every distinct **combination of attribute values** on an instrument creates its own time series: it is aggregated separately in memory, exported on every push, and stored and indexed by the backend forever. The total number of combinations (the _cardinality_) is the product of the value counts of all attributes — it grows multiplicatively, and it is the primary driver of both metrics cost and query complexity. A histogram multiplies this further: each series carries one counter **per bucket**.

A good metric therefore has a **small, bounded, and predictable** set of attribute values, known roughly at design time:

- ✅ `http.request.method` (~9 values), `http.response.status_code` (~60, in practice ~10), `http.route` (bounded by the number of routes), `credential_format` (a handful of enum variants).
- ❌ Anything identifier-like or unbounded: credential/offer/aggregate IDs, UUIDs, DIDs, subject or holder identifiers, session/correlation IDs, raw URL paths or query strings, timestamps, free-text error messages, anything derived from user input.

Rules of thumb:

- **Estimate the product before adding an attribute.** `method × route × status` for the HTTP histogram is on the order of a few hundred series — fine. Adding a per-tenant ID with 10,000 tenants turns it into millions.
- **Attributes are for grouping, not for lookup.** If you would ever filter a metric down to a _single_ entity ("what happened to credential X?"), that question belongs to a **trace or log**, which carry per-request context for free — not to a metric. Metrics answer aggregate questions ("how many? how fast? how often?").
- **Use `http.route`, never the raw path.** The `track_metrics` middleware records the matched route pattern (`/v0/credentials/{credential_id}`) instead of the concrete URL precisely to keep the value set bounded. Follow the same principle for any attribute: record the _category_, not the _instance_.
- **Map open-ended inputs to a closed set.** Bucket error details into a small `error.type`, cap enums coming from external input to known values plus `"other"`.
- **The SDK will defend itself, at the cost of your data.** The OpenTelemetry SDK caps the number of series per instrument (cardinality limit, default 2000); once exceeded, additional combinations are folded into a single series with the `otel.metric.overflow` attribute — your metric silently stops being attributable. Treat hitting that limit as a bug in the metric's design, not something to raise the limit for.

## Adding a new metric: step by step

### 1. Create an instrument from the global meter

Always resolve the meter through `opentelemetry::global::meter(...)`. This decouples the recording site from the initialization: when OpenTelemetry is not activated, the global provider is a no-op and recording is essentially free. There is no separate registration step — building the instrument defines the metric.

```rust
use opentelemetry::KeyValue;

let counter = opentelemetry::global::meter("unicore")
    .u64_counter("offers_created")
    .with_description("The number of credential offers created.")
    .build();
```

Available instrument kinds on the meter: `u64_counter`, `f64_histogram`, `u64_gauge`, `i64_up_down_counter`, and their observable (callback-based) variants.

Naming: use lowercase dot-separated names and check the [semantic conventions](https://opentelemetry.io/docs/specs/semconv/) first — if a convention exists for what you are measuring (as it does for HTTP), use its metric name, unit, and attribute names so that off-the-shelf dashboards work.

### 2. Record values

```rust
counter.add(1, &[KeyValue::new("credential_format", "sd_jwt")]);
```

- Recording is synchronous and in-memory; the periodic exporter pushes the aggregated state to the collector in the background.
- Keep the attribute value set small and bounded — see [Cardinality](#cardinality-what-makes-a-good-metric) before adding any attribute.
- For histograms, set explicit buckets with `.with_boundaries(...)` where the defaults do not fit — see `agent_api_http/src/metrics.rs`.
- If the same instrument is recorded on a hot path, build it once and cache it (e.g. in a `OnceLock`, as `track_metrics` does) instead of rebuilding it on every call. Make sure the first access happens **after** startup so the cached instrument is bound to the real meter provider, not the no-op default.

### 3. Verify locally

Spin up the local all-in-one OTel stack (OTel Collector, Tempo, Loki, Prometheus, and a Grafana UI behind one OTLP endpoint) from [`agent_application/docker/telemetry/compose.yaml`](../../agent_application/docker/telemetry/compose.yaml):

```sh
docker compose -f agent_application/docker/telemetry/compose.yaml up -d

OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317" cargo run
```

Then exercise the code path that records your metric and check it in Grafana (default `http://localhost:3001`) under **Explore → Prometheus**.
