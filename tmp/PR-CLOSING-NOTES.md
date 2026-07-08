# OpenTelemetry PR cleanup notes

Drafts for closing PRs #303, #307, and #308, superseded by the work on `feat/init-opentelemetry`,
plus a follow-up issue for the one capability not carried over. Copy-paste and adjust as you see fit.

---

## Closing comment for #307 (replace `init-tracing-opentelemetry` with `opentelemetry-appender-tracing`)

> Closing in favor of `feat/init-opentelemetry`, which implements the same explicit telemetry setup
> (hand-rolled OTLP glue in `agent_application/src/telemetry.rs`, no `init-tracing-opentelemetry`,
> console log always on, guard-based provider shutdown, tracing init early in `run()`) and extends it:
>
> - Activation is driven purely by the standard environment variables — the presence of
>   `OTEL_EXPORTER_OTLP_ENDPOINT` enables export instead of a config flag; `OTEL_SDK_DISABLED`,
>   `OTEL_SERVICE_NAME`, `OTEL_RESOURCE_ATTRIBUTES` and `OTEL_{TRACES,LOGS,METRICS}_EXPORTER` are respected.
> - Metrics are exported as a third signal next to traces and logs.
> - The OTel log bridge filters out the export pipeline's own events to prevent feedback loops.
>
> Thanks — this PR's structure (telemetry module, `TelemetryGuard`, layer composition) served as the
> blueprint for the final implementation.

## Closing comment for #308 (Introduce OpenTelemetry for logs, traces, and metrics)

> Closing in favor of `feat/init-opentelemetry`, which covers everything here — all three pillars via
> OTLP/gRPC, `RUST_LOG`/`EnvFilter` respected across all layers, graceful provider shutdown — with two
> deliberate differences:
>
> - Activation via the standard `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable instead of
>   `UNICORE__OPENTELEMETRY__*` config, keeping the OTel wiring out of `agent_shared`'s configuration.
> - A proper `Resource` is attached, so `service.name` resolves from `OTEL_SERVICE_NAME` (falling back
>   to `unicore`) instead of `unknown_service`.
>
> The `.env.example` documentation from this PR has been ported over.

## Closing comment for #303 (initialize `axum` instrumentation with OpenTelemetry)

> Closing: this is built on `init-tracing-opentelemetry`, which we decided against
> (see `feat/init-opentelemetry` for the replacement), and it removes the request/response body
> logging middleware that we want to keep.
>
> The `axum-tracing-opentelemetry` instrumentation itself (semconv HTTP spans, incoming `traceparent`
> extraction, trace id response header) is still valuable and is tracked as a follow-up: #<issue-number>.

---

## Follow-up issue draft

**Title:** Adopt semconv HTTP spans and incoming trace-context extraction

**Body:**

> `feat/init-opentelemetry` exports the existing `TraceLayer` spans (named "HTTP Request") to OTLP and
> registers the W3C `TraceContext` propagator, but nothing extracts trace context from incoming requests
> yet. PR #303 explored this via `axum-tracing-opentelemetry` before being closed.
>
> Follow-up, on top of the telemetry module in `agent_application`:
>
> - [ ] Add `axum-tracing-opentelemetry`'s `OtelAxumLayer` **alongside** (not replacing) the existing
>       trace/body-logging layers, so HTTP root spans follow OTel semantic conventions
>       (span name `GET /v0/offers`, `http.*`/`url.*` attributes).
> - [ ] Incoming `traceparent`/`tracestate` headers join UniCore spans to the caller's distributed trace
>       (comes with `OtelAxumLayer`; requires the globally registered propagator, already in place).
> - [ ] Add `OtelInResponseLayer` so responses carry the trace id in a header, allowing API errors to be
>       correlated with traces in Jaeger.
> - [ ] Consider demoting the existing "HTTP Request" span or merging its fields into the semconv span
>       to avoid double spans per request.
>
> Only `axum-tracing-opentelemetry` is needed as a dependency — **not** `init-tracing-opentelemetry` —
> since the tracer provider is already registered by `agent_application/src/telemetry.rs`.
