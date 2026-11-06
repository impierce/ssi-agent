#!/usr/bin/env bash
#
# Fuzz the HTTP API against its own generated OpenAPI specification.
#
# Boots a disposable UniCore instance (agent + Postgres) with Docker Compose on a network without a
# route to the outside, seeds it with resources, points Schemathesis at `agent_api_http/openapi.yaml`
# (or `openapi-full.yaml` with `FUZZ_SPEC=full`), and tears everything down again. The specification
# is the one committed in this repository; CI already guarantees it is in sync with the
# `#[utoipa::path(...)]` annotations, so there is nothing to regenerate here.
#
# What this looks for: places where the implementation and the specification disagree — undocumented
# status codes, responses that do not match their schema, wrong content types, valid input rejected,
# invalid input accepted, 500s. It does not look for authorization problems: open-source UniCore has
# no authorization checks by design (they are extension points implemented downstream), so the
# `ignored_auth` check is excluded rather than left to report the obvious.
#
# Usage:
#   ./scripts/fuzz-openapi.sh                     # full run of the published specification
#   FUZZ_SPEC=full ./scripts/fuzz-openapi.sh      # also fuzz the standardized protocol endpoints
#   FUZZ_MAX_EXAMPLES=20 ./scripts/fuzz-openapi.sh
#   FUZZ_MAX_TIME=3600 ./scripts/fuzz-openapi.sh  # repeat fuzzing and stateful phases for an hour
#   FUZZ_COMMAND=fuzz FUZZ_MAX_TIME=3600 ./scripts/fuzz-openapi.sh  # continuous multi-step fuzzing
#   FUZZ_SEED=12345 ./scripts/fuzz-openapi.sh     # reproduce a previous run
#   FUZZ_KEEP_UP=1 ./scripts/fuzz-openapi.sh      # leave the stack running for inspection
#
# Reports are written to `reports/fuzz/` (JUnit XML, HAR, NDJSON, agent log, seeded identifiers).

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
DOCKER_DIR="$REPO_ROOT/agent_application/docker"
REPORTS_DIR="$REPO_ROOT/reports/fuzz"
FUZZ_SPEC="${FUZZ_SPEC:-published}"
FUZZ_COMMAND="${FUZZ_COMMAND:-run}"

case "$FUZZ_SPEC" in
    published) FUZZ_SPEC_FILE="openapi.yaml" ;;
    full) FUZZ_SPEC_FILE="openapi-full.yaml" ;;
    *)
        echo "Error: FUZZ_SPEC must be 'published' or 'full', got '$FUZZ_SPEC'." >&2
        exit 1
        ;;
esac

case "$FUZZ_COMMAND" in
    run | fuzz) ;;
    *)
        echo "Error: FUZZ_COMMAND must be 'run' or 'fuzz', got '$FUZZ_COMMAND'." >&2
        exit 1
        ;;
esac

SPEC_PATH="$REPO_ROOT/agent_api_http/$FUZZ_SPEC_FILE"

FUZZ_MAX_EXAMPLES="${FUZZ_MAX_EXAMPLES:-50}"
FUZZ_MAX_TIME="${FUZZ_MAX_TIME:-}"
FUZZ_READY_TIMEOUT="${FUZZ_READY_TIMEOUT:-120}"
FUZZ_KEEP_UP="${FUZZ_KEEP_UP:-0}"

if [ "$FUZZ_COMMAND" = "fuzz" ] && [ -z "$FUZZ_MAX_TIME" ]; then
    echo "Error: FUZZ_COMMAND=fuzz runs until stopped; set FUZZ_MAX_TIME." >&2
    exit 1
fi

export FUZZ_SPEC_FILE

compose() {
    docker compose --project-directory "$DOCKER_DIR" -f "$DOCKER_DIR/compose.fuzz.yaml" "$@"
}

# Run as the invoking user so the reports on the host are not owned by root. That leaves the image's
# own home and working directory unwritable, hence the switch to /tmp.
tool() {
    compose run --rm --user "$(id -u):$(id -g)" --workdir /tmp -e HOME=/tmp "$@"
}

teardown() {
    compose logs --no-color ssi-agent > "$REPORTS_DIR/agent.log" 2>&1 || true

    if [ "$FUZZ_KEEP_UP" = "1" ]; then
        echo "==> FUZZ_KEEP_UP=1, leaving the stack running."
        echo "    The agent is only reachable inside the compose network, e.g.:"
        echo "      docker compose --project-directory $DOCKER_DIR -f $DOCKER_DIR/compose.fuzz.yaml run --rm --entrypoint python3 schemathesis -c \"import urllib.request; print(urllib.request.urlopen('http://ssi-agent:3033/v0/list-all-templates').read().decode())\""
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
mkdir -p "$REPORTS_DIR/seed"

trap teardown EXIT

echo "==> Building the agent image"
compose build ssi-agent

echo "==> Starting the agent and its event store"
compose up -d ssi-agent

echo "==> Waiting for readiness and seeding resources"
if ! tool --entrypoint python3 schemathesis /scripts/fuzz-seed.py http://ssi-agent:3033 /reports/seed "$FUZZ_READY_TIMEOUT"; then
    echo "Error: the agent did not become ready or could not be seeded." >&2
    compose logs --no-color ssi-agent >&2 || true
    exit 1
fi

schemathesis_args=(
    --config-file /config/schemathesis.toml
    "$FUZZ_COMMAND" /spec/openapi.yaml
    --url http://ssi-agent:3033
    --workers auto
    --mode all
    --checks all
    # Open-source UniCore has no authorization checks by design.
    --exclude-checks ignored_auth
    # Nothing carries over between runs of a disposable container, and disabling the example
    # database keeps the container from needing a writable working directory.
    --generation-database none
    # One broken operation should not hide the state of the others.
    --continue-on-failure
    # Explicit paths: the default report filenames are timestamped, and CI wants stable ones.
    --report-junit-path /reports/junit.xml
    --report-har-path /reports/traffic.har
    --report-ndjson-path /reports/events.ndjson
)

if [ "$FUZZ_COMMAND" = "run" ]; then
    schemathesis_args+=(--max-examples "$FUZZ_MAX_EXAMPLES")
fi

if [ -n "$FUZZ_MAX_TIME" ]; then
    schemathesis_args+=(--max-time "$FUZZ_MAX_TIME")
fi

if [ -n "${FUZZ_SEED:-}" ]; then
    schemathesis_args+=(--seed "$FUZZ_SEED")
fi

if [ -n "${FUZZ_EXTRA_ARGS:-}" ]; then
    # Word splitting is intended: this is a pass-through for ad-hoc flags.
    # shellcheck disable=SC2206
    schemathesis_args+=(${FUZZ_EXTRA_ARGS})
fi

echo "==> Fuzzing ${FUZZ_SPEC_FILE} (${FUZZ_COMMAND}, ${FUZZ_MAX_EXAMPLES} examples per operation${FUZZ_MAX_TIME:+, ${FUZZ_MAX_TIME}s})"
set +e
tool schemathesis "${schemathesis_args[@]}"
fuzz_exit=$?
set -e

echo
echo "==> Finished with exit code ${fuzz_exit}"
echo "    Specification: $SPEC_PATH"
echo "    Reports:       $REPORTS_DIR"

exit "$fuzz_exit"
