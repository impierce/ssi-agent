"""Schemathesis hooks used by `scripts/fuzz-openapi.sh`, loaded through `SCHEMATHESIS_HOOKS` (`compose.fuzz.yaml`)."""

import schemathesis

# Compact JWTs (signed presentations, status list tokens) are plain text, so they are handed to the `string` schema as
# they are. Schemathesis counts `application/jwt` as a JSON media type and its built-in JSON deserializer is consulted
# first, so that deserializer is replaced rather than a separate one registered for `application/jwt`.
JWT_MEDIA_TYPES = ("application/jwt", "application/statuslist+jwt")


def _is_jwt(response):
    content_type = (response.headers.get("content-type") or [""])[0]
    return content_type.split(";")[0].strip().lower() in JWT_MEDIA_TYPES


@schemathesis.deserializer("application/json")
def json_or_compact_jwt(ctx, response):
    if _is_jwt(response):
        return response.content.decode()
    return response.json()


@schemathesis.deserializer("application/statuslist+jwt")
def compact_jwt(ctx, response):
    return response.content.decode()
