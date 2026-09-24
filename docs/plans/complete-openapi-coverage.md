# Complete OpenAPI coverage

## Status

Proposed implementation plan for review.

## Context

UniCore currently generates `agent_api_http/openapi.yaml` from the `utoipa`
annotations registered by `agent_api_http::v0::openapi::ApiDoc`. That document
contains the management API and `GET /public/templates`, but it does not describe
the protocol routes or the application metadata and probe routes.

This plan addresses the requirements captured in:

- [ssi-agent issue #285](https://github.com/impierce/ssi-agent/issues/285), which
  asks whether consumer-facing and standardized protocol endpoints should be
  represented separately.
- [ssi-agent issue #351](https://github.com/impierce/ssi-agent/issues/351), which
  defines the eventual runtime split between the management and public/protocol
  APIs.
- `TODO.md`, which requires every endpoint to appear in a complete OpenAPI
  document while retaining a smaller, less protocol-heavy default document.

The documentation split comes first. Splitting the HTTP server is a separate,
follow-up change because it alters deployment and security behavior.

## Decisions

### Generated artifacts

Generate two OpenAPI documents from Rust types:

| Artifact | Contents |
| --- | --- |
| `agent_api_http/openapi.yaml` | Every currently documented operation, plus `GET /public/sponsoring-configuration`, `GET /version`, `GET /info`, `GET /healthz`, and `GET /readyz` |
| `agent_api_http/openapi-full.yaml` | Everything in `openapi.yaml`, plus every standardized protocol endpoint |

The existing operations in `openapi.yaml` remain unchanged. The five explicitly
requested general-purpose endpoints are additive changes to that artifact.

### Composition instead of YAML merging

Do not generate independent YAML fragments and merge the serialized files.
Compose `utoipa::openapi::OpenApi` documents in Rust and serialize only the final
documents. This retains compile-time schema checks, detects conflicting paths and
component names earlier, and avoids a second YAML merge implementation.

### Generator ownership

The final generators should live in `agent_application` because that crate owns
`/version`, `/info`, `/healthz`, and `/readyz` and already depends on
`agent_api_http`. Moving those handlers into `agent_api_http`, or adding fake
documentation-only handlers there, would invert or blur the existing dependency
direction.

The document hierarchy should be:

```text
agent_api_http::ApiDoc
    management operations
    /public/templates
    /public/sponsoring-configuration
            |
            v
agent_application::PublishedApiDoc
    agent_api_http::ApiDoc
    /version
    /info
    /healthz
    /readyz
            |
            v
agent_application::FullApiDoc
    PublishedApiDoc
    agent_api_http::ProtocolApi
```

`PublishedApiDoc` generates `openapi.yaml`; `FullApiDoc` generates
`openapi-full.yaml`.

## Endpoint inventory

### Added to both documents

| Method | Path | Owner |
| --- | --- | --- |
| `GET` | `/public/sponsoring-configuration` | `agent_api_http::public` |
| `GET` | `/version` | `agent_application::metadata` |
| `GET` | `/info` | `agent_application::metadata` |
| `GET` | `/healthz` | `agent_application::probes` |
| `GET` | `/readyz` | `agent_application::probes` |

`/readyz` is included even though issue #351 does not name it: it is a real
UniCore route, and the complete-coverage requirement permits no omissions.

### Added only to the full document

| Method | Path | Area |
| --- | --- | --- |
| `GET` | `/.well-known/did.json` | DID |
| `GET` | `/.well-known/did-configuration.json` | DID Configuration |
| `GET` | `/.well-known/oauth-authorization-server` | OAuth 2.0 metadata |
| `GET` | `/.well-known/openid-credential-issuer` | OpenID4VCI metadata |
| `POST` | `/openid4vci/credential` | OpenID4VCI |
| `POST` | `/openid4vci/nonce` | OpenID4VCI |
| `POST` | `/openid4vci/notification` | OpenID4VCI |
| `GET` | `/openid4vci/credential-offer/{offer_id}` | OpenID4VCI |
| `GET` | `/ietf-oauth-token-status-list/{path}` | OAuth Token Status List |
| `GET` | `/vct/{credential_configuration_id}/{version}` | SD-JWT VC type metadata |
| `GET` | `/auth/consent` | Authorization UI |
| `POST` | `/auth/consent` | Authorization UI |
| `POST` | `/auth/par` | OAuth PAR / interactive authorization |
| `GET` | `/auth/authorize` | OAuth authorization |
| `POST` | `/auth/token` | OAuth token |
| `GET` | `/credential_offer` | OpenID4VCI holder callback |
| `GET` | `/linked-verifiable-presentations/{presentation_id}` | Linked VP |
| `GET` | `/request/{request_id}` | OID4VP/SIOPv2 request object |
| `POST` | `/redirect` | OID4VP/SIOPv2 response |

This is 19 protocol operations. Together with the five shared operations, it
accounts for the 24 operations currently routed but absent from the generated
specification.

## Phase 1: add OpenAPI schemas to `openid4vc`

Complete and merge the standalone plan in
[`openid4vc-utoipa-phase-1.md`](./openid4vc-utoipa-phase-1.md) before modifying
the UniCore protocol annotations.

After it lands:

1. Update the `openid4vc` revision in the workspace `Cargo.toml`.
2. Keep the `oid4vci` `utoipa` feature enabled.
3. Let that feature enable the new `oid4vc-core/utoipa` feature transitively.
4. Run the UniCore workspace tests before starting endpoint annotations, so an
   upstream schema regression is separated from local documentation changes.

## Phase 2: add the five shared endpoints to `openapi.yaml`

### Public sponsoring configuration

1. Derive `utoipa::ToSchema` for `SponsoringConfiguration`.
2. Add `#[utoipa::path]` to `sponsoring_configuration` with operation ID
   `sponsoring_configuration`.
3. Register it in `public::openapi::PublicApi` alongside
   `get_public_templates`.
4. Document the `200`, `404`, and `500` responses that the handler currently
   produces.

### Metadata and probes

1. Derive `ToSchema` for `Version` and `Info`.
2. Ensure `ApplicationProfile`, nested by `Info`, has a schema or an explicit
   schema representation.
3. Add path annotations with operation IDs `version`, `info`, `healthz`, and
   `readyz`, matching their handler names.
4. Document `/readyz` with both `200` and `503`; document `/healthz` as an empty
   `200` response.
5. Define `OperationalApi` in `agent_application` and register these four
   operations.

### Published document

1. Define `PublishedApiDoc` in `agent_application` by composing the existing
   `agent_api_http::v0::openapi::ApiDoc` and `OperationalApi` at the root path.
2. Move responsibility for writing `agent_api_http/openapi.yaml` to a generator
   test in `agent_application`.
3. Resolve the output through `env!("CARGO_MANIFEST_DIR")`; do not rely on the
   process working directory.
4. Retain the existing title, license, external documentation, server, and
   semantic-release version patching.
5. Remove the old writer only after a regression test proves that the only
   OpenAPI diff is the five intended operations and their schemas.

## Phase 3: annotate protocol endpoints

Add `#[utoipa::path]` annotations to all 19 protocol operations and collect them
in `agent_api_http::ProtocolApi`.

Each operation must specify:

- An operation ID that is snake_case and exactly matches the handler name. Rename
  a handler when necessary rather than introducing a second HTTP-facing name.
- The actual request encoding: JSON, query, URL-encoded form, HTML, compact JWT,
  or bytes as appropriate.
- Success and error status codes currently emitted by the handler.
- Required bearer authentication and relevant response headers.
- A standards-oriented tag: `DID`, `OAuth 2.0`, `OpenID4VCI`, `OID4VP / SIOPv2`,
  `Status List`, or `SD-JWT VC`.

### Local schema work

Keep UniCore-specific wire types in UniCore:

- Derive `ToSchema` for `AuthorizationRequestDto` and use the upstream
  `InteractiveAuthorizationFollowUpRequest` instead of duplicating its fields.
- Derive schemas/parameters for `ConsentForm` and the consent query.
- Replace the untyped documentation shape of `/credential_offer` with a local
  query DTO containing the mutually exclusive `credential_offer` and
  `credential_offer_uri` fields. The runtime handler may continue to validate
  their exclusivity.
- Return or at least document the upstream `NonceResponse` instead of an ad hoc
  `serde_json::Value`.
- Create a local documentation DTO for `/redirect` with SIOPv2 (`state`,
  `id_token`) and OID4VP (`state`, `vp_token`) alternatives. Do not force the
  generic `oid4vc_core::AuthorizationResponse<E>` through `utoipa` solely for
  this endpoint.
- Reuse the custom DID document OpenAPI schema code already present in
  `agent_identity`.
- Add local schema adapters for Domain Linkage Configuration and SD-JWT VC type
  metadata if their external crates do not expose `ToSchema`.
- Describe compact JWT and gzip status-list bodies as strings/binary content
  with their exact media types rather than pretending they are JSON objects.

### Full document

Define `FullApiDoc` in `agent_application` as `PublishedApiDoc` plus
`agent_api_http::ProtocolApi`, and generate
`agent_api_http/openapi-full.yaml` from it.

The full document is a strict operation-level superset of `openapi.yaml`.

## Phase 4: generation and completeness tests

Add tests that operate on the generated `OpenApi` values before serialization:

1. Build the set of `(method, path)` operations in `PublishedApiDoc`.
2. Build the same set for `FullApiDoc`.
3. Assert that the published set is a subset of the full set.
4. Assert that the full set equals an explicit audited route manifest.
5. Assert that the difference between the two sets is exactly the 19 protocol
   operations listed above.
6. Assert globally unique operation IDs.
7. Assert snake_case operation IDs and handler-name parity.
8. Parse both serialized YAML files as OpenAPI after generation.
9. Generate each file twice and assert stable output.

Axum does not provide a stable public API for enumerating every registered
route. The audited manifest is therefore an intentional test boundary. Any
future router change must update both its documentation and the manifest.

Update the documented generation command so one command regenerates both files.
`cargo test generate_openapi_spec` may remain that command if its filter runs
both generator tests deterministically.

## Phase 5: split the runtime listeners

Implement issue #351 after the documentation work has landed.

1. Extract `management_router`, `protocol_router`, and a backward-compatible
   combined `app` router.
2. Add optional `management_url` configuration.
3. When `management_url` is absent, serve the combined router exactly as today.
4. When `management_url` is present:
   - Serve only protocol, public, metadata, and probe endpoints on
     `application_url`.
   - Serve `/v0/*` management endpoints on `management_url`.
5. Reject colliding listener addresses during configuration validation.
6. Test that `/v0/*` is unavailable from the public listener when the split is
   enabled.
7. Document firewall and Kubernetes `NetworkPolicy` implications.

This phase must not be bundled into the schema or OpenAPI-generation PRs because
it changes the externally reachable security boundary.

## Delivery sequence

1. `openid4vc`: schema feature and types from the extracted Phase 1 plan.
2. `ssi-agent`: bump the pinned `openid4vc` revision only.
3. `ssi-agent`: add the five shared operations and transfer generation to
   `PublishedApiDoc`.
4. `ssi-agent`: annotate the 19 protocol operations and add `FullApiDoc`.
5. `ssi-agent`: add completeness tests and documentation/collection consumers.
6. `ssi-agent`: implement the optional two-listener runtime split.

Keeping the revision bump isolated makes upstream integration failures easy to
identify. Keeping the listener split last makes the documentation changes safe
to release independently.

## Acceptance criteria

- `openapi.yaml` contains its existing operations plus the five shared
  operations and no standardized protocol operations.
- `openapi-full.yaml` contains every operation served by the complete UniCore
  application router.
- The full document differs from the published document by exactly the audited
  19 protocol operations.
- Every operation has a unique, snake_case ID matching its handler.
- Request bodies and responses use their real media types.
- The two generated files are deterministic and parse as valid OpenAPI.
- `cargo fmt --all` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo test --workspace` passes.
- The runtime behavior and listener topology remain unchanged until Phase 5.
