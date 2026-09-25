# Add `utoipa` schemas for UniCore protocol endpoints

## Status

Completed in `impierce/openid4vc` revision `42b37c8` and consumed by UniCore.

## Goal

Expose opt-in OpenAPI schemas for the OpenID4VCI wire types used by UniCore's
public protocol endpoints. The work must remain feature-gated so downstream
users that do not generate OpenAPI documents do not acquire a mandatory
`utoipa` dependency.

The first consumer is `impierce/ssi-agent`, which was pinned to revision
`be4e047` when this plan was written and now consumes the completed work at
revision `42b37c8`.

## Scope

This phase covers:

- `oid4vci` request, response, metadata, credential-offer, and error models.
- The `oid4vc-core` claim-path types nested by OID4VCI models.
- An `IntoParams` representation for the authorization-by-reference query.
- Schema-focused tests and documentation.

This phase does not cover:

- HTTP handlers or OpenAPI path definitions.
- `oid4vp` and `siopv2` schemas. UniCore will use a local wire DTO for its
  combined redirect endpoint instead of exposing the generic
  `AuthorizationResponse<E>` type.
- Builder, service, validator, cryptographic, or internal state types.
- Changes to serialized protocol behavior.

## Feature wiring

### `oid4vc-core/Cargo.toml`

1. Add an empty-by-default feature set if the crate does not already have one.
2. Add `utoipa = ["dep:utoipa"]`.
3. Add `utoipa` as an optional workspace dependency.

### `oid4vci/Cargo.toml`

Change the existing feature from:

```toml
utoipa = ["dep:utoipa"]
```

to:

```toml
utoipa = ["dep:utoipa", "oid4vc-core/utoipa"]
```

All derives and schema attributes must use `cfg_attr(feature = "utoipa", ...)`.
The default feature set must remain unchanged.

## Required schema inventory

The list below is the minimum type graph required by UniCore. It includes direct
request/response types and their non-primitive transitive fields.

### Authorization server and issuer metadata

Files:

- `oid4vci/src/credential_issuer/authorization_server_metadata.rs`
- `oid4vci/src/credential_issuer/credential_issuer_metadata.rs`
- `oid4vci/src/credential_issuer/credential_configurations_supported.rs`

Types:

- `AuthorizationServerMetadata`
- `CredentialIssuerMetadata`
- `CredentialResponseEncryption`
- `BatchCredentialIssuance`
- `BatchSize`
- `CredentialConfigurationsSupportedObject`
- `CredentialMetadata`
- `AlgIdentifier`
- `ClaimDescription`
- `ClaimDescriptionDisplay`
- `Logo`
- `Image`
- `CredentialConfigurationsSupportedDisplay`

Schema requirements:

- URI fields remain `string` with URI format.
- `BatchSize` is an integer with minimum `2`.
- Optional fields remain optional and preserve `skip_serializing_none` behavior.
- The `credential_configurations_supported` map uses the complete credential
  configuration schema as its value.
- Color fields remain strings; do not invent validation that runtime code does
  not enforce.

### Credential format profiles

Files:

- `oid4vci/src/credential_format_profiles/mod.rs`
- The format modules below that directory.

Types:

- `CredentialFormats<WithParameters>`
- `Parameters<F>`
- `JwtVcJsonParameters`
- `jwt_vc_json::CredentialDefinition`
- `JwtVcJsonLdParameters`
- `jwt_vc_json_ld::CredentialDefinition`
- `LdpVcParameters`
- `ldp_vc::CredentialDefinition`
- `MsoMdocParameters`
- `DcSdJwtParameters`
- `VcSdJwtParameters`
- `vc_sd_jwt::CredentialDefinition`

Update the `credential_format!` macro so generated parameter types receive the
feature-gated schema implementation. Generated credential payload types are not
part of the minimum consumer graph and should only be annotated if doing so is
effectively free and accurately represents their wire format.

The schema must preserve the internally tagged `format` representation and the
flattened format-specific parameters. A manual `ToSchema` implementation for
`CredentialFormats<WithParameters>` is preferable to a derive that produces an
incorrect generic or nested representation.

### Proof metadata

File: `oid4vci/src/proof.rs`

Types:

- `KeyProofMetadata`
- `ProofType`

`Proof` and `ProofOfPossession` are not currently required by UniCore's endpoint
schema because `CredentialRequest` uses `Proofs`.

### Credential request and response

Files:

- `oid4vci/src/credential_request.rs`
- `oid4vci/src/proofs.rs`
- `oid4vci/src/credential_response.rs`

Types:

- `CredentialRequest`
- `CredentialIdentifierOrCredentialConfigurationId`
- `Proofs`
- `CredentialResponse`
- `CredentialResponseType`
- `CredentialResponseObject`

The generated schema must preserve the flattened credential identifier choice
and the untagged immediate/deferred response alternatives.

### Credential offer

File: `oid4vci/src/credential_offer.rs`

Types:

- `CredentialOfferParameters`
- `CredentialConfigurationIds`
- `Grants`
- `AuthorizationCode`
- `PreAuthorizedCode`

Already feature-gated at revision `be4e047`:

- `TxCodeConstraints`
- `InputMode`

Schema requirements:

- `CredentialConfigurationIds` is a non-empty array of strings.
- The pre-authorized grant retains its full URN property name.
- `TxCodeConstraints.description` remains a string with maximum length `300`.
- `TxCodeConstraints.length` retains its `0..=255` constraints.
- `Description` does not require a standalone component while its field is
  represented explicitly as a string.

`CredentialOffer` itself is not required for the current UniCore path schemas:
the offer retrieval endpoint returns `CredentialOfferParameters`, while the
holder callback accepts two query-string alternatives. It may be added if a
second consumer needs the deep-link enum as a component.

### Authorization and token flow

Files:

- `oid4vci/src/authorization_request.rs`
- `oid4vci/src/authorization_details.rs`
- `oid4vci/src/token_request.rs`
- `oid4vci/src/token_response.rs`
- `oid4vci/src/wallet/mod.rs`

Types:

- `AuthorizationRequest`
- `CodeChallengeMethod`
- `AuthorizationDetailsObject`
- `AuthorizationDetailsClaim`
- `OpenidCredential`
- `TokenRequest`
- `TokenResponse`
- `PushedAuthorizationResponse`
- `AuthorizationRequestByReference`

In addition to `ToSchema`, derive or implement `IntoParams` for
`AuthorizationRequestByReference`, whose two fields are query parameters on the
authorization endpoint.

The `TokenRequest` schema must preserve its `grant_type` discriminator and the
exact pre-authorized-code URN.

### Interactive authorization

Files:

- `oid4vci/src/interactive_authorization_request.rs`
- `oid4vci/src/interactive_authorization_response.rs`

Types:

- `InteractionType`
- `InteractiveAuthorizationRequest`
- `InteractiveAuthorizationFollowUpRequest`
- `InteractiveAuthorizationResponse`
- `InteractiveAuthorizationStatus`

`InteractiveAuthorizationErrorResponse` is not part of the minimum graph until
UniCore returns it from the endpoint. It can be included in this PR if it is
documented as a generally supported public wire type rather than as speculative
UniCore behavior.

### Nonce and notification

Files:

- `oid4vci/src/nonce_response.rs`
- `oid4vci/src/notification_request.rs`

Types:

- `NonceResponse`
- `NotificationRequest`
- `NotificationEvent`

### Protocol errors

File: `oid4vci/src/errors.rs`

Types:

- `OID4VCError<T>`
- `TokenErrorResponse`
- `CredentialErrorResponse`
- `NotificationErrorResponse`

The generic error schema must expose `error` and optional `error_description`
without losing the concrete error enum. Add concrete test aliases if the derive
cannot prove the generic bounds cleanly.

`AuthorizationErrorResponse`, `DeferredCredentialErrorResponse`, and
`InteractiveAuthorizationErrorResponse` are outside the minimum UniCore graph.
They may be annotated for API consistency, but their inclusion must not expand
the PR into unrelated error redesign.

### Claim paths in `oid4vc-core`

File: `oid4vc-core/src/claim_path_pointer.rs`

Types:

- `ClaimPathPointer`
- `ClaimPathElement`

`ClaimPathPointer` needs an explicit schema matching its wire representation:
a non-empty array whose elements are a string, a non-negative integer, or null.
Do not expose the nutype wrapper as an object.

`ClaimValue` and `ClaimValues` are not in the required type graph.

## Documentation requirements

For every exposed schema:

1. Add or retain a Rust documentation comment linking the relevant standards
   section where one exists.
2. Document fields whose meaning is not evident from their Rust name.
3. Add schema examples only when the example is normative, already used in a
   repository test, or clearly synthetic.
4. Express existing runtime constraints such as minimum sizes, maximum lengths,
   URI formats, and non-empty arrays.
5. Do not add schema constraints that deserialization or constructors do not
   enforce.
6. Verify that `serde` renames, flattening, untagged enums, and tagged enums are
   reflected in the schema.

## Implementation sequence

### Commit 1: feature plumbing

- Add the optional `oid4vc-core/utoipa` feature and dependency.
- Propagate it from `oid4vci/utoipa`.
- Add a minimal compile test proving default builds do not require the feature
  and all-feature builds enable both crates.

### Commit 2: core and scalar schemas

- Implement schemas for claim paths and constrained newtypes.
- Add focused assertions for array shape and numeric/string constraints.

### Commit 3: OID4VCI request/response schemas

- Add credential offer, request, response, token, nonce, notification, and error
  schemas.
- Add `AuthorizationRequestByReference::IntoParams`.

### Commit 4: metadata and credential-format schemas

- Add authorization-server and issuer metadata schemas.
- Extend the format macro and implement the concrete credential-format union.
- Test flattening, tags, component references, and all supported format variants.

### Commit 5: interactive authorization and aggregate fixture

- Add the interactive request/response schemas.
- Add the complete consumer fixture described below.
- Fill documentation gaps discovered by the fixture review.

The exact commit split may be compressed, but feature plumbing should remain
separate from the large mechanical annotation change.

## Tests

Create an `#[cfg(all(test, feature = "utoipa"))]` fixture that derives one
`OpenApi` document with schemas rooted at:

- `AuthorizationServerMetadata`
- `CredentialIssuerMetadata`
- `CredentialRequest`
- `CredentialResponse`
- `CredentialOfferParameters`
- `AuthorizationRequest`
- `InteractiveAuthorizationRequest`
- `InteractiveAuthorizationFollowUpRequest`
- `InteractiveAuthorizationResponse`
- `TokenRequest`
- `TokenResponse`
- `NonceResponse`
- `NotificationRequest`
- `OID4VCError<TokenErrorResponse>`
- `OID4VCError<CredentialErrorResponse>`
- `OID4VCError<NotificationErrorResponse>`

Assertions should verify:

- Every referenced component resolves.
- Credential formats expose all six currently modeled format identifiers.
- Flattened and untagged types match representative serialized examples.
- URN-renamed grant fields and variants appear verbatim.
- Nutype schemas expose their primitive/array wire form rather than wrapper
  objects.
- The schema serializes to both JSON and YAML without errors.

Also run the existing serde tests with and without the feature. Adding schemas
must not change serialization.

## Validation commands

Run the repository's documented equivalents of:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features
```

If the repository's minimum-supported Rust version is tested in CI, verify that
the selected `utoipa` version supports it.

## Acceptance criteria

- `oid4vc-core` and `oid4vci` compile with default features and no mandatory
  `utoipa` dependency.
- Enabling `oid4vci/utoipa` enables every schema required by the aggregate
  fixture, including `oid4vc-core` claim paths.
- The complete fixture has no unresolved component references.
- Generated schemas preserve the actual `serde` wire representation.
- Constraints on nutypes and constrained fields match runtime behavior.
- Existing serialization and protocol tests remain unchanged and pass.
- Public schema types and non-obvious fields have useful documentation.
- UniCore can annotate its protocol handlers without substituting the listed
  OID4VCI types with generic OpenAPI objects.

## Consumer handoff

After merge, provide the commit SHA to the UniCore change. The first downstream
commit should only update the pinned `openid4vc` revision and run the UniCore
workspace checks. Endpoint annotations belong in later commits so failures can
be attributed cleanly to either dependency integration or local OpenAPI work.
