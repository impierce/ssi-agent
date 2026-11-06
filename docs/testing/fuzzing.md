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
`agent_application/docker/compose.fuzz.yaml`, seeds it with resources (see below), runs Schemathesis
inside the same compose network, and tears everything down. It needs Docker and nothing else — no
local Python or Schemathesis installation.

The stack uses its own compose project name and publishes no port, so a run never disturbs a local
development instance.

| Variable | Default | Purpose |
| --- | --- | --- |
| `FUZZ_SPEC` | `published` | `published` fuzzes `openapi.yaml`; `full` fuzzes `openapi-full.yaml`, which adds the standardized protocol endpoints |
| `FUZZ_COMMAND` | `run` | `run` executes the examples, coverage, fuzzing and stateful phases once per operation; `fuzz` generates multi-step operation sequences until `FUZZ_MAX_TIME` |
| `FUZZ_MAX_EXAMPLES` | `50` | Test cases generated per operation (`run` only) |
| `FUZZ_MAX_TIME` | _(none)_ | Time budget in seconds; with `run`, the fuzzing and stateful phases repeat until it is spent. Required for `fuzz` |
| `FUZZ_SEED` | _(none)_ | Reproduce an earlier run |
| `FUZZ_KEEP_UP` | `0` | Leave the stack running afterwards for inspection |
| `FUZZ_EXTRA_ARGS` | _(none)_ | Extra Schemathesis flags, e.g. `--include-path /v0/credentials` |
| `FUZZ_READY_TIMEOUT` | `120` | Seconds to wait for `/readyz` before seeding |
| `SCHEMATHESIS_VERSION` | `v4.33.1` | Pinned `schemathesis/schemathesis` image tag |

To narrow a run down to one operation while fixing a finding:

```bash
FUZZ_SEED=12345 FUZZ_EXTRA_ARGS="--include-path /v0/create-new-template" ./scripts/fuzz-openapi.sh
```

For an in-depth run, give it time instead of examples:

```bash
FUZZ_MAX_TIME=3600 FUZZ_MAX_EXAMPLES=500 ./scripts/fuzz-openapi.sh
FUZZ_COMMAND=fuzz FUZZ_MAX_TIME=3600 ./scripts/fuzz-openapi.sh
```

Reports land in `reports/fuzz/` (gitignored): `junit.xml` for the summary, `traffic.har` for the
exact requests and responses, `events.ndjson` for the raw event stream, and `seed/` for the seeded
identifiers. Schemathesis prints a reproducing `curl` command for every finding, and the run's seed,
so any failure can be replayed. Output sanitization is off, so the reproductions show the request
that was actually sent.

To render the same Markdown breakdown CI produces:

```bash
python3 scripts/fuzz-summary.py reports/fuzz/junit.xml
```

## Seeded resources

Most operations take the identifier of an existing resource. Without any, they answer `404` and their
handler logic is never reached. Before fuzzing, `scripts/fuzz-seed.py` creates a few published
templates (some with constant values only, as public offers require), catalogs, offers, credentials,
public offers, authorization requests and an in-memory key provider, and writes their identifiers,
together with the DID documents created at startup, to `reports/fuzz/seed/`.
`agent_application/docker/schemathesis.toml` binds the identifier parameters (`templateId`,
`catalogId`, `offerId`, `catalog_id`, ...) to these dictionaries with a probability of 0.5, so half of
the generated values refer to resources that exist and the other half stays random.

Holder credentials, holder offers, presentations, connections and reissuances are not seeded: they
need a remote issuer, wallet or agent.

## Configuration

`agent_application/docker/schemathesis.toml` holds what the command line cannot express:

- The dictionaries above.
- No `NUL` characters: the API rejects them in front of every handler on purpose
  (`request#unsupported-character`), because the event stores cannot hold them.
- No output sanitization: the fuzz instance only holds throwaway data and secrets.
- `422` counts as an accepted answer to a schema-valid request (`positive_data_acceptance`): a request
  can match the schema and still refer to a template that is not published or a resource that does
  not exist, which the API answers with `422`. A `400` for schema-valid input stays a finding.
- `agent_application/docker/schemathesis_hooks.py`: a deserializer for compact JWT responses
  (`application/jwt`), which Schemathesis would otherwise parse as JSON.

`agent_application/docker/fuzz.config.yaml` enables every switch that hides documented operations
(`serve_openapi_enabled`, `feature.kms`), every DID method and signing algorithm that works without a
remote service, and disables both event publishers.

## Hermeticity

A fuzzer sends arbitrary data to every operation, including URLs and domains that would make the
agent call third parties: credential-issuer metadata (`POST /v0/connections`), linked domain
verification, sending offers to a `targetUrl`, resolving `did:web`. The compose project therefore runs
on an internal Docker network without a route to the outside. Every operation is fuzzed, and outbound
calls fail at the network boundary instead of reaching whatever host the generated input names. For
the same reason, readiness checks and seeding run from the Schemathesis container inside that network,
and no port is published.

`agent_application/docker/fuzz.config.yaml` also disables both event publishers and the `did:web`
method, so a run does not depend on them.

## Published or full specification

The protocol endpoints are defined by external standards and rarely change, so the nightly run
fuzzes the published specification only. Their handlers and error mapping can still change, so run
the full specification after changing them, and before a release:

```bash
FUZZ_SPEC=full ./scripts/fuzz-openapi.sh
```

In CI, pick `full` for the `spec` input of a manual run.

## In CI

The `Fuzz OpenAPI` workflow runs nightly and on demand, never on pull requests. Property-based
testing explores different inputs on every run, so a failure is a finding to triage rather than a
verdict on whichever commit happened to trigger it — gating pull requests on it would produce flakes
and, soon after, a disabled workflow.

Trigger a run from the Actions tab; `spec`, `command`, `max_examples`, `max_time`, `seed`, and
`extra_args` are exposed as inputs.
Findings appear in the job summary, with the full reports attached as a `fuzz-reports` artifact.
