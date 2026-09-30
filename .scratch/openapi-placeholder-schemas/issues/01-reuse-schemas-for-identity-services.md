# Replace the placeholder schemas of `ServiceResponse`

Status: ready-for-agent

## Context

`ServiceResponse` in `agent_api_http/src/v0/identity/services/mod.rs` is returned by
`GET /v0/services` (`list_identity_services`) and `GET /v0/services/{service_id}`
(`get_identity_service`). Two of its fields are documented as untyped objects, each with a TODO:

```rust
/// TODO: Replace this generic object schema with a schema for `identity_document::service::Service`.
#[schema(value_type = Option<Object>)]
service: Option<DocumentService>,
/// TODO: Replace this generic object schema with a schema for `DomainLinkageConfiguration`.
#[schema(value_type = Option<Object>)]
resource: Option<ServiceResource>,
```

The complete OpenAPI coverage work (`docs/plans/complete-openapi-coverage.md`) added schemas that
describe both types, so the placeholders can be replaced without new schema work:

- `agent_identity::document::openapi::DidService` describes a DID Core service
  (`id`, `type`, `serviceEndpoint`), which is what `identity_document::service::Service` serializes.
- `agent_api_http::v0::identity::well_known::did_configuration::DomainLinkageConfigurationSchema`
  (published as `DomainLinkageConfiguration`) describes the DID Configuration resource.

## What to do

1. `service`: use `#[schema(value_type = Option<DidService>)]`.
2. `resource`: `ServiceResource` is an externally tagged enum (serde's default), so it serializes as
   `{"LinkedDomains": { "@context": …, "linked_dids": […] }}`, not as the configuration itself.
   Describe that shape, e.g. with a small documentation-only schema (an object with a
   `LinkedDomains` property that references `DomainLinkageConfiguration`), and reference it with
   `value_type`. Verify the shape against a real response first.
3. Remove the two TODO comments once their placeholders are replaced.
4. Regenerate both documents with `cargo test generate_openapi_spec`.

## Acceptance criteria

- `ServiceResponse.service` references `DidService` and `ServiceResponse.resource` describes the
  externally tagged `LinkedDomains` shape in `openapi.yaml` and `openapi-full.yaml`.
- No other operation or schema changes in the generated documents.
- `component_names_are_unambiguous` and the other OpenAPI tests pass, together with `cargo fmt --all`,
  `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --workspace`.

## Notes

- This changes `openapi.yaml`, and therefore the generated TypeScript types for `ServiceResponse`.
- Out of scope: the three placeholders in `agent_api_http/src/v0/verification/authorization_requests.rs`
  (OpenID authorization request types, `DecodedVpToken`, `DcqlQuery`). They need new schemas,
  partly upstream in `openid4vc` (`oid4vp`), and deserve their own issue.
