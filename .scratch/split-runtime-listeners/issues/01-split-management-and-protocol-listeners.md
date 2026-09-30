# Serve the management API and the protocol API on separate listeners

Status: needs-triage

## Context

This is Phase 5 of `docs/plans/complete-openapi-coverage.md` and implements
[ssi-agent issue #351](https://github.com/impierce/ssi-agent/issues/351). Phases 1–4 documented every
endpoint: `agent_api_http/openapi.yaml` describes the management API plus the shared public,
metadata and probe endpoints, and `agent_api_http/openapi-full.yaml` adds the 19 standardized
protocol operations (tag `Protocol`, collected in `agent_api_http::v0::openapi::ProtocolApi`).

Today a single listener serves everything: `agent_api_http::app` merges all routers and
`agent_application::serve` binds it to the port of `application_url`. The management endpoints
(`/v0/*`) are therefore reachable wherever the protocol endpoints are, which have to be public for
wallets, verifiers and resolvers.

This changes the externally reachable security boundary, so it ships as its own PR, separate from
the documentation work.

## What to do

1. In `agent_api_http/src/lib.rs`, extract a `management_router` (all `/v0/*` routes, including the
   `/v0/events` SSE route that is merged outside the trace layer) and a `protocol_router` (protocol,
   `/public/*`, well-known routes). Keep `app` as the backward-compatible combination of both.
   Preserve today's layering: the actor-extraction middleware, the trace and body-logging layers,
   and the nesting under the base path of `application_url` (well-known routes stay at the root).
2. Add an optional `management_url: Option<Url>` to the application config in `agent_shared`
   (`UNICORE__MANAGEMENT_URL`).
3. Without `management_url`, serve the combined `app` exactly as today.
4. With `management_url`:
   - serve protocol, public, metadata and probe endpoints on `application_url`;
   - serve `/v0/*` on `management_url`, via a second `start_server` task in
     `agent_application/src/lib.rs`.
5. Reject a `management_url` whose listener address collides with `application_url` during
   configuration validation.
6. Test that `/v0/*` answers `404` on the public listener when the split is enabled, and that the
   combined router is unchanged when it is not.
7. Document the new setting in `docs/configuration/`, including firewall and Kubernetes
   `NetworkPolicy` implications.

## Decisions to make first

- **Probes and metadata:** serve `/healthz`, `/livez`, `/readyz`, `/version` and `/info` on both
  listeners, or only on the public one? Kubernetes probes usually target one port.
- **`/openapi.yaml`:** which listener serves it when `UNICORE__SERVE_OPENAPI_ENABLED=true`, and
  should the management listener serve the management document only?
- **CORS:** `cors_enabled` applies per `start_server` call today. Should it apply to both listeners?
- **Base path:** does `management_url` carry its own base path, analogous to `application_url`?
- **Naming in #351:** the issue predates the current documents. It calls the management spec
  `openapi-generated.yaml` and uses `openapi-full.yaml` for the protocol-only spec. Today
  `openapi-full.yaml` is a superset of `openapi.yaml`. Decide whether the split also needs a
  protocol-only or management-only document, and update #351 accordingly.

## Acceptance criteria

- Without `management_url`, routing and behavior are unchanged.
- With `management_url`, `/v0/*` is only reachable on the management listener, and protocol, public,
  metadata and probe endpoints stay reachable on `application_url`.
- Colliding listener addresses are rejected at startup with a clear configuration error.
- Configuration docs describe the setting and its network implications.
- `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings` and
  `cargo test --workspace` pass.
