#!/usr/bin/env bash
# Regenerates the Bruno collection in ./gen from ../openapi.yaml.
#
# Run manually whenever the spec changes:
#   agent_api_http/bruno/generate.sh
#
# What it does:
#   1. `bru import openapi` converts the spec into a fresh collection tree
#      (one .bru file per request, grouped into folders by OpenAPI tag).
#   2. patch-collection.mjs re-injects the things OpenAPI has no concept of:
#      API-key auth, chained-request variables, and the post-response scripts
#      that extract response data into those variables.
#   3. The result replaces every generated request/folder in ./gen.
#      Everything under environments/ is left alone except the auto-generated
#      "Local development" entry, so any other environments you've added by
#      hand (staging, prod, ...) survive a regen.
#
# This collection is meant to be regenerated wholesale, not hand-edited.
# If a new endpoint needs response-extraction scripting or a pre-filled path
# param, add a rule to patch-collection.mjs (matched by method + path, not by
# filename) and re-run this script.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPEC_FILE="$SCRIPT_DIR/../openapi.yaml"
GENERATED_DIR="$SCRIPT_DIR/gen"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
# bru import nests its output under <output>/<collection-name> if <output>
# already exists, but writes directly into <output> if it doesn't. Remove the
# empty dir mktemp just created so we get the direct (non-nested) layout.
rmdir "$TMP_DIR"

if ! command -v npx >/dev/null 2>&1; then
  echo "error: npx is required (ships with Node.js)" >&2
  exit 1
fi

echo "Importing $SPEC_FILE via bru CLI..."
npx --yes @usebruno/cli import openapi \
  --source "$SPEC_FILE" \
  --output "$TMP_DIR" \
  --collection-name "UniCore HTTP API" \
  --collection-format bru \
  --group-by tags

echo "Patching auth, variables, and post-response scripts..."
node "$SCRIPT_DIR/patch-collection.mjs" "$TMP_DIR"

echo "Syncing into $GENERATED_DIR ..."
# Replace every generated folder/request, but leave any environment file alone
# except the one bru import always regenerates from the spec's `servers:` list.
mkdir -p "$GENERATED_DIR"
find "$GENERATED_DIR" -mindepth 1 -maxdepth 1 \
  ! -name 'environments' \
  -exec rm -rf {} +

cp -R "$TMP_DIR"/. "$GENERATED_DIR"/

echo "Done. Review the diff (git status / git diff) before committing."
