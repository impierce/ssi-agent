use agent_shared::config::LogFormat;
use opentelemetry::trace::TracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Guard that ensures OpenTelemetry providers are properly shut down when dropped.
pub struct TelemetryGuard {
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(tracer_provider) = self.tracer_provider.take() {
            if let Err(e) = tracer_provider.shutdown() {
                eprintln!("Failed to shut down tracer provider: {e}");
            }
        }
        if let Some(logger_provider) = self.logger_provider.take() {
            if let Err(e) = logger_provider.shutdown() {
                eprintln!("Failed to shut down logger provider: {e}");
            }
        }
    }
}

/// Initialize the tracing subscriber with console output and optional OpenTelemetry export.
///
/// - Console logging is always enabled (JSON or text format based on `log_format`).
/// - When `opentelemetry_enabled` is `true`, traces and logs are exported to an
///   OpenTelemetry collector via OTLP (configured through standard `OTEL_EXPORTER_*`
///   environment variables).
///
/// Returns a [`TelemetryGuard`] that must be held for the lifetime of the application
/// to ensure proper shutdown of OpenTelemetry providers.
pub fn init_telemetry(log_format: &LogFormat, opentelemetry_enabled: bool) -> TelemetryGuard {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    if opentelemetry_enabled {
        init_with_opentelemetry(log_format, env_filter)
    } else {
        init_console_only(log_format, env_filter)
    }
}

/// Set up a console-only tracing subscriber (no OpenTelemetry export).
fn init_console_only(log_format: &LogFormat, env_filter: EnvFilter) -> TelemetryGuard {
    let registry = tracing_subscriber::registry().with(env_filter);

    match log_format {
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        LogFormat::Text => registry
            .with(tracing_subscriber::fmt::layer())
            .init(),
    }

    TelemetryGuard {
        tracer_provider: None,
        logger_provider: None,
    }
}

/// Set up a tracing subscriber with console output and OpenTelemetry trace + log export.
fn init_with_opentelemetry(log_format: &LogFormat, env_filter: EnvFilter) -> TelemetryGuard {
    // --- Trace exporter (OTLP via gRPC) ---
    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to build OTLP span exporter");

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .build();

    let otel_trace_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer_provider.tracer("ssi-agent"));

    // --- Log exporter (OTLP via gRPC) ---
    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to build OTLP log exporter");

    let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .build();

    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider);

    // --- Compose the subscriber ---
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(otel_trace_layer)
        .with(otel_log_layer);

    match log_format {
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        LogFormat::Text => registry
            .with(tracing_subscriber::fmt::layer())
            .init(),
    }

    TelemetryGuard {
        tracer_provider: Some(tracer_provider),
        logger_provider: Some(logger_provider),
    }
}
