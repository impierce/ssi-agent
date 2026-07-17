# OpenTelemetry

This Docker Compose file fires up a local OpenTelemetry test stack for traces, logs, and metrics.

It uses the [Grafana LGTM all-in-one image](https://github.com/grafana/docker-otel-lgtm):

- Tempo (traces)
- Loki (logs)
- Prometheus (metrics)

It also includes a pre-provisioned Grafana UI with dashboards for all three signals.

## Usage

```bash
# From the repository root:
docker compose -f agent_application/docker/telemetry/compose.yaml up -d

# From the `agent_application/docker/telemetry` folder:
docker compose up -d
```

Set the `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable to activate OpenTelemetry export in UniCore:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
```

Visit http://localhost:3000 (Grafana) and go to the `Explore` tab.
