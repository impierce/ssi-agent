use agent_shared::config::LogFormat;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider, propagation::TraceContextPropagator, trace::SdkTracerProvider,
    Resource,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Environment variables that activate OpenTelemetry export when present.
///
/// Follows the [OTLP Exporter specification](https://opentelemetry.io/docs/specs/otel/protocol/exporter/): the
/// general endpoint applies to all signals, the signal-specific ones take precedence for their respective signal.
const OTLP_ENDPOINT_ENV_VARS: [&str; 4] = [
    "OTEL_EXPORTER_OTLP_ENDPOINT",
    "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
    "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
];

/// OTLP transport protocol, selected through the standard `OTEL_EXPORTER_OTLP_PROTOCOL` environment
/// variables.
#[derive(Clone, Copy)]
enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
}

/// The OTLP transport protocol for a signal, respecting the signal-specific
/// `OTEL_EXPORTER_OTLP_{TRACES,LOGS,METRICS}_PROTOCOL` environment variable over the general
/// `OTEL_EXPORTER_OTLP_PROTOCOL` (default: `grpc`).
///
/// # Panics
///
/// Panics when the configured protocol is not one of the supported values `grpc` and `http/protobuf`,
/// since silently falling back would export to an endpoint speaking a different protocol.
fn otlp_protocol(signal_env_var: &str) -> OtlpProtocol {
    let value = std::env::var(signal_env_var)
        .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .unwrap_or_default();

    match value.to_ascii_lowercase().as_str() {
        "" | "grpc" => OtlpProtocol::Grpc,
        "http/protobuf" => OtlpProtocol::HttpProtobuf,
        other => panic!("Unsupported OTLP protocol `{other}`, expected `grpc` or `http/protobuf`"),
    }
}

/// Guard that shuts down the OpenTelemetry providers (flushing any pending export batches) when dropped.
#[derive(Default)]
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(tracer_provider) = self.tracer_provider.take() {
            if let Err(error) = tracer_provider.shutdown() {
                eprintln!("Failed to shut down OpenTelemetry tracer provider: {error}");
            }
        }
        if let Some(logger_provider) = self.logger_provider.take() {
            if let Err(error) = logger_provider.shutdown() {
                eprintln!("Failed to shut down OpenTelemetry logger provider: {error}");
            }
        }
        if let Some(meter_provider) = self.meter_provider.take() {
            if let Err(error) = meter_provider.shutdown() {
                eprintln!("Failed to shut down OpenTelemetry meter provider: {error}");
            }
        }
    }
}

/// Whether OpenTelemetry export should be activated.
///
/// Activation is driven purely by the standard OpenTelemetry environment variables: the presence of an OTLP
/// endpoint enables export, [`OTEL_SDK_DISABLED`](https://opentelemetry.io/docs/specs/otel/configuration/sdk-environment-variables/)
/// force-disables it.
fn otel_export_enabled() -> bool {
    let sdk_disabled = std::env::var("OTEL_SDK_DISABLED").is_ok_and(|value| value.eq_ignore_ascii_case("true"));

    !sdk_disabled
        && OTLP_ENDPOINT_ENV_VARS
            .iter()
            .any(|var| std::env::var(var).is_ok_and(|value| !value.is_empty()))
}

/// Whether the exporter for an individual signal is enabled, respecting the standard
/// `OTEL_{TRACES,LOGS,METRICS}_EXPORTER` environment variables (default: `otlp`).
///
/// Only the `otlp` exporter is supported; any other value (e.g. `none`) disables the signal. This allows
/// disabling individual signals when the collector does not support them (e.g. Jaeger only accepts traces).
fn signal_enabled(exporter_env_var: &str) -> bool {
    std::env::var(exporter_env_var).map_or(true, |value| value.eq_ignore_ascii_case("otlp"))
}

/// Resource describing this service, respecting `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES`.
///
/// Only when no service name is provided through the environment does the name default to `unicore`.
fn resource() -> Resource {
    let mut builder = Resource::builder();

    if std::env::var("OTEL_SERVICE_NAME").is_err() {
        builder = builder.with_service_name("unicore");
    }

    builder.build()
}

/// Initialize the global tracing subscriber with console output and optional OpenTelemetry export.
///
/// - Console logging is always enabled (JSON or text format based on `log_format`), driven by the `RUST_LOG`
///   environment variable (defaulting to `info`).
/// - When an OTLP endpoint is configured through the standard `OTEL_EXPORTER_OTLP_*` environment variables,
///   traces, logs and metrics are additionally exported via OTLP, using gRPC (default) or HTTP/protobuf as
///   selected by `OTEL_EXPORTER_OTLP_PROTOCOL`. All other standard `OTEL_*` environment variables (service
///   name, headers, timeouts, ...) are respected by the exporters.
///
/// Returns a [`TelemetryGuard`] that must be held for the lifetime of the application to ensure the
/// OpenTelemetry providers are flushed and shut down properly.
pub fn init_telemetry(log_format: &LogFormat) -> TelemetryGuard {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    // The console format layers are mutually exclusive; `Option`s keep the subscriber composition uniform.
    let (json_layer, text_layer) = match log_format {
        LogFormat::Json => (Some(tracing_subscriber::fmt::layer().json()), None),
        LogFormat::Text => (None, Some(tracing_subscriber::fmt::layer())),
    };

    let mut guard = TelemetryGuard::default();
    let mut otel_trace_layer = None;
    let mut otel_log_layer = None;

    if otel_export_enabled() {
        let resource = resource();

        // Traces: `tracing` spans are bridged to OTel spans and exported via OTLP.
        if signal_enabled("OTEL_TRACES_EXPORTER") {
            let span_exporter = match otlp_protocol("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL") {
                OtlpProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder().with_tonic().build(),
                OtlpProtocol::HttpProtobuf => opentelemetry_otlp::SpanExporter::builder().with_http().build(),
            }
            .expect("Failed to build the OTLP span exporter");
            let tracer_provider = SdkTracerProvider::builder()
                .with_batch_exporter(span_exporter)
                .with_resource(resource.clone())
                .build();

            otel_trace_layer = Some(tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("unicore")));
            guard.tracer_provider = Some(tracer_provider);
        }

        // Logs: `tracing` events are bridged to OTel log records and exported via OTLP. Events emitted by the
        // OTLP export pipeline itself are excluded to prevent a feedback loop on export errors.
        if signal_enabled("OTEL_LOGS_EXPORTER") {
            let log_exporter = match otlp_protocol("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL") {
                OtlpProtocol::Grpc => opentelemetry_otlp::LogExporter::builder().with_tonic().build(),
                OtlpProtocol::HttpProtobuf => opentelemetry_otlp::LogExporter::builder().with_http().build(),
            }
            .expect("Failed to build the OTLP log exporter");
            let logger_provider = SdkLoggerProvider::builder()
                .with_batch_exporter(log_exporter)
                .with_resource(resource.clone())
                .build();

            otel_log_layer = Some(OpenTelemetryTracingBridge::new(&logger_provider).with_filter(
                tracing_subscriber::filter::filter_fn(|metadata| {
                    let target = metadata.target();
                    !["opentelemetry", "tonic", "h2", "tower", "hyper", "reqwest"]
                        .iter()
                        .any(|noisy| target.starts_with(noisy))
                }),
            ));
            guard.logger_provider = Some(logger_provider);
        }

        // Metrics: the global meter provider exports via OTLP on a periodic interval.
        if signal_enabled("OTEL_METRICS_EXPORTER") {
            let metric_exporter = match otlp_protocol("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL") {
                OtlpProtocol::Grpc => opentelemetry_otlp::MetricExporter::builder().with_tonic().build(),
                OtlpProtocol::HttpProtobuf => opentelemetry_otlp::MetricExporter::builder().with_http().build(),
            }
            .expect("Failed to build the OTLP metric exporter");
            let meter_provider = SdkMeterProvider::builder()
                .with_periodic_exporter(metric_exporter)
                .with_resource(resource)
                .build();

            opentelemetry::global::set_meter_provider(meter_provider.clone());
            guard.meter_provider = Some(meter_provider);
        }

        // Propagate trace context on outgoing requests using the W3C `traceparent` header.
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
    }

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .with(text_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();

    guard
}
