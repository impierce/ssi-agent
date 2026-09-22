# Fuzzing the HTTP API

UniCore's OpenAPI specification (`agent_api_http/openapi.yaml`) is generated from the
`#[utoipa::path(...)]` annotations on the handlers. Generation keeps the two files in sync, but it
cannot tell whether an annotation is *true*: an operation annotated as returning `200` can still
return an undocumented `422`, a response can diverge from the schema it claims, and valid input can
be rejected. [Schemathesis](https://schemathesis.readthedocs.io) finds those disagreements by
generating test cases from the specification and checking every response against it.

## What it checks

All of Schemathesis' checks run except `ignored_auth`. Open-source UniCore deliberately has no
authorization checks — it ships `NoActorExtractor` and `AllowAllAuthorizationChecker`, with the real
policy supplied downstream through those extension points — so an authorization check here would only
report the intended design. Everything else is in scope:

| Check | Catches |
| --- | --- |
| `not_a_server_error` | Input that reaches a `500` |
| `status_code_conformance` | Status codes the specification does not document |
| `content_type_conformance` | Content types the specification does not document |
| `response_headers_conformance` | Documented headers that are missing |
| `response_schema_conformance` | Response bodies that do not match their schema |
| `negative_data_rejection` | Invalid input that is accepted anyway |
| `positive_data_acceptance` | Valid input that is rejected anyway |
| `missing_required_header` | Required headers that are not actually enforced |
| `unsupported_method` | Methods answered on paths that should not support them |
| `use_after_free` | Resources still readable after deletion |
| `ensure_resource_availability` | Resources not readable right after creation |

## Running it

```bash
./scripts/fuzz-openapi.sh
```

The script builds the agent image, starts it against a throwaway Postgres via
`agent_application/docker/compose.fuzz.yaml`, waits for `/readyz`, runs Schemathesis inside the same
compose network, and tears everything down. It needs Docker and nothing else — no local Python or
Schemathesis installation.

The stack uses its own compose project name and publishes the agent on port `3099` rather than
`3033`, so a run never disturbs a local development instance.

| Variable | Default | Purpose |
| --- | --- | --- |
| `FUZZ_MAX_EXAMPLES` | `50` | Test cases generated per operation |
| `FUZZ_SEED` | _(none)_ | Reproduce an earlier run |
| `FUZZ_KEEP_UP` | `0` | Leave the stack running afterwards for inspection |
| `FUZZ_EXTRA_ARGS` | _(none)_ | Extra Schemathesis flags, e.g. `--include-path /v0/credentials` |
| `UNICORE_FUZZ_PORT` | `3099` | Host port the agent is published on |
| `SCHEMATHESIS_VERSION` | `4.9.4` | Pinned Schemathesis image tag |

To narrow a run down to one operation while fixing a finding:

```bash
FUZZ_SEED=12345 FUZZ_EXTRA_ARGS="--include-path /v0/create-new-template" ./scripts/fuzz-openapi.sh
```

Reports land in `reports/fuzz/` (gitignored): `junit.xml` for the summary, `traffic.har` for the
exact requests and responses, `events.ndjson` for the raw event stream. Schemathesis prints a
reproducing `curl` command for every finding, and the run's seed, so any failure can be replayed.

To render the same Markdown breakdown CI produces:

```bash
python3 scripts/fuzz-summary.py reports/fuzz/junit.xml
```

## Hermeticity

A fuzzer sends arbitrary data to every operation it is given, including operations that would
normally reach third parties. Two measures keep a run self-contained:

- `agent_application/docker/fuzz.config.yaml` disables both event publishers and the `did:web`
  method, so nothing leaves the compose project through configuration.
- The runner excludes the three operations that reach out regardless of configuration:
  `GET /v0/verify-linked-domains` (resolves each linked origin's DID configuration and does a CNAME
  lookup), `POST /v0/connections` (fetches credential-issuer metadata from the supplied URL), and
  `POST /v0/connections/sync-connection` (fetches state from the remote agent).

## In CI

The `Fuzz OpenAPI` workflow runs nightly and on demand, never on pull requests. Property-based
testing explores different inputs on every run, so a failure is a finding to triage rather than a
verdict on whichever commit happened to trigger it — gating pull requests on it would produce flakes
and, soon after, a disabled workflow.

Trigger a run from the Actions tab; `max_examples`, `seed`, and `extra_args` are exposed as inputs.
Findings appear in the job summary, with the full reports attached as a `fuzz-reports` artifact.
