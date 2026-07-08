I have three PRs open in impierce/ssi-agent related to OpenTelemetry:

- https://github.com/impierce/ssi-agent/pull/308
- https://github.com/impierce/ssi-agent/pull/307
- https://github.com/impierce/ssi-agent/pull/303

I would like you to analyze them all and implement OpenTelementry properly in the currently branch. You do not need to merge them into this branch, but just use them for inspiration if you see fit.

I would like to follow standards as close as possible, so the presence alone of `OTEL_EXPORTER_OTLP_ENDPOINT` should activate it. If it's not present, OTel shouldn't be collected. Also respect other standard envs, such as `OTEL_SERVICE_NAME`.

The supposedly simple `init-tracing-opentelemetry` crate didn't work out for me, so we need to "glue it together" ourselves. I also would like to keep a friendly console log for developers during development, ideally nothing changes when the `OTLP_ENDPOINT` is not set. If it set, I'd still like to keep a console-friendly log without too much tracing information.

I can validate your implementation locally against my running Jaeger instance (Docker).
