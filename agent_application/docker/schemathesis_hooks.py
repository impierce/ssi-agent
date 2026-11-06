"""Schemathesis hooks used by `scripts/fuzz-openapi.sh`, loaded through `SCHEMATHESIS_HOOKS` (`compose.fuzz.yaml`)."""

import schemathesis


# Compact JWTs (signed presentations, status list tokens) are plain text. Without a deserializer, Schemathesis parses
# the body as JSON to check it against the `string` schema and reports the JWT as invalid JSON.
@schemathesis.deserializer("application/jwt", "application/statuslist+jwt")
def compact_jwt(ctx, response):
    return response.content.decode()
