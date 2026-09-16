# Monitoring

UniCore offers a variety of monitoring options to ensure a healthy deployment and successful operations.

## Probes

UniCore implements conventional probe endpoints to monitor the availability and health of the service. [Standard Kubernetes probes](https://kubernetes.io/docs/concepts/configuration/liveness-readiness-startup-probes/) are served at the following endpoints:

- `/livez`: Liveness probe
- `/healthz`: Backwards-compatible alias for the liveness probe
- `/readyz`: Readiness probe. It returns `503 Service Unavailable` while the application is
  starting, replaying projections, waiting for a writer lease, or shutting down.

## Metadata

The following endpoints provide metadata about the service itself and allow for more detailed deployment monitoring:

- `/version`: Returns the version and the git commit hash
- `/info`: Returns the version, the git commit hash, a Docker build timestamp and the container uptime

:::warning

Although the `/version` and `/info` endpoints do not contain sensitive data, they are **not intended to be exposed to the public internet**.

:::

## Metrics

Metrics are exported via **OpenTelemetry** (OTLP push), activated by setting the standard `OTEL_EXPORTER_OTLP_*` environment variables. There is no `/metrics` endpoint to scrape.

See [Metrics](../metrics/README.md) for the available metrics, how to activate the export, and how to add new metrics.
