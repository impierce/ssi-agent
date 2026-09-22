#!/usr/bin/env bash
#
# Fuzz the HTTP API against its own generated OpenAPI specification.
#
# Boots a disposable UniCore instance (agent + Postgres) with Docker Compose, points Schemathesis
# at `agent_api_http/openapi.yaml`, and tears everything down again. The specification is the one
# committed in this repository; CI already guarantees it is in sync with the `#[utoipa::path(...)]`
# annotations, so there is nothing to regenerate here.
#
# What this looks for: places where the implementation and the specification disagree — undocumented
# status codes, responses that do not match their schema, wrong content types, valid input rejected,
# invalid input accepted, 500s. It does not look for authorization problems: open-source UniCore has
# no authorization checks by design (they are extension points implemented downstream), so the
# `ignored_auth` check is excluded rather than left to report the obvious.
#
# Usage:
#   ./scripts/fuzz-openapi.sh                 # full run
#   FUZZ_MAX_EXAMPLES=20 ./scripts/fuzz-openapi.sh
#   FUZZ_SEED=12345 ./scripts/fuzz-openapi.sh # reproduce a previous run
#   FUZZ_KEEP_UP=1 ./scripts/fuzz-openapi.sh  # leave the stack running for inspection
#
# Reports are written to `reports/fuzz/` (JUnit XML, HAR, NDJSON).

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
DOCKER_DIR="$REPO_ROOT/agent_application/docker"
REPORTS_DIR="$REPO_ROOT/reports/fuzz"
SPEC_PATH="$REPO_ROOT/agent_api_http/openapi.yaml"

# Not 3033: that is where a local development instance usually listens.
UNICORE_FUZZ_PORT="${UNICORE_FUZZ_PORT:-3099}"
FUZZ_MAX_EXAMPLES="${FUZZ_MAX_EXAMPLES:-50}"
FUZZ_READY_TIMEOUT="${FUZZ_READY_TIMEOUT:-120}"
FUZZ_KEEP_UP="${FUZZ_KEEP_UP:-0}"

export UNICORE_FUZZ_PORT

# Operations that would reach outside the compose project even with event publishers disabled.
# Everything else is fair game.
EXCLUDED_PATHS=(
    # Resolves each linked origin's DID configuration over the network, plus a CNAME lookup.
    "/v0/verify-linked-domains"
    # Fetches the latest state of a connection from the remote agent.
    "/v0/connections/sync-connection"
)

EXCLUDED_OPERATION_IDS=(
    # POST /v0/connections fetches credential-issuer metadata from the submitted URL.
    "add_connection"
)

compose() {
    docker compose --project-directory "$DOCKER_DIR" -f "$DOCKER_DIR/compose.fuzz.yaml" "$@"
}

teardown() {
    if [ "$FUZZ_KEEP_UP" = "1" ]; then
        echo "==> FUZZ_KEEP_UP=1, leaving the stack running."
        echo "    Agent:    http://localhost:${UNICORE_FUZZ_PORT}"
        echo "    Teardown: docker compose --project-directory $DOCKER_DIR -f $DOCKER_DIR/compose.fuzz.yaml down -v"
        return
    fi
    echo "==> Tearing down"
    compose down -v --remove-orphans >/dev/null 2>&1 || true
}

if [ ! -f "$SPEC_PATH" ]; then
    echo "Error: OpenAPI specification not found at '$SPEC_PATH'." >&2
    echo "Generate it with: cargo test generate_openapi_spec" >&2
    exit 1
fi

mkdir -p "$REPORTS_DIR"
rm -rf "${REPORTS_DIR:?}"/*

trap teardown EXIT

echo "==> Building the agent image"
compose build ssi-agent

echo "==> Starting the agent and its event store"
compose up -d ssi-agent

echo "==> Waiting for readiness on http://localhost:${UNICORE_FUZZ_PORT}/readyz"
deadline=$((SECONDS + FUZZ_READY_TIMEOUT))
until curl -fsS -o /dev/null "http://localhost:${UNICORE_FUZZ_PORT}/readyz"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
        echo "Error: the agent did not become ready within ${FUZZ_READY_TIMEOUT}s." >&2
        compose logs --no-color ssi-agent >&2 || true
        exit 1
    fi
    if [ -z "$(compose ps -q ssi-agent)" ]; then
        echo "Error: the agent container exited during startup." >&2
        compose logs --no-color ssi-agent >&2 || true
        exit 1
    fi
    sleep 2
done
echo "==> Agent is ready"

schemathesis_args=(
    run /spec/openapi.yaml
    # Reached over the compose network, so the published host port stays free for inspection.
    --url http://ssi-agent:3033
    --workers auto
    --mode all
    --checks all
    # Open-source UniCore has no authorization checks by design.
    --exclude-checks ignored_auth
    --max-examples "$FUZZ_MAX_EXAMPLES"
    # Nothing carries over between runs of a disposable container, and disabling the example
    # database keeps the container from needing a writable working directory.
    --generation-database none
    # One broken operation should not hide the state of the other 53.
    --continue-on-failure
    # Explicit paths: the default report filenames are timestamped, and CI wants stable ones.
    --report-junit-path /reports/junit.xml
    --report-har-path /reports/traffic.har
    --report-ndjson-path /reports/events.ndjson
)

for path in "${EXCLUDED_PATHS[@]}"; do
    schemathesis_args+=(--exclude-path "$path")
done

for operation_id in "${EXCLUDED_OPERATION_IDS[@]}"; do
    schemathesis_args+=(--exclude-operation-id "$operation_id")
done

if [ -n "${FUZZ_SEED:-}" ]; then
    schemathesis_args+=(--seed "$FUZZ_SEED")
fi

if [ -n "${FUZZ_EXTRA_ARGS:-}" ]; then
    # Word splitting is intended: this is a pass-through for ad-hoc flags.
    # shellcheck disable=SC2206
    schemathesis_args+=(${FUZZ_EXTRA_ARGS})
fi

excluded_operations=$((${#EXCLUDED_PATHS[@]} + ${#EXCLUDED_OPERATION_IDS[@]}))
echo "==> Fuzzing with ${FUZZ_MAX_EXAMPLES} examples per operation (${excluded_operations} operations excluded)"
set +e
# Run as the invoking user so the reports on the host are not owned by root. That leaves the
# image's own home and working directory unwritable, hence the switch to /tmp.
compose run --rm \
    --user "$(id -u):$(id -g)" \
    --workdir /tmp \
    -e HOME=/tmp \
    schemathesis "${schemathesis_args[@]}"
fuzz_exit=$?
set -e

echo
echo "==> Finished with exit code ${fuzz_exit}"
echo "    Specification: $SPEC_PATH"
echo "    Reports:       $REPORTS_DIR"

exit "$fuzz_exit"
