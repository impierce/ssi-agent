# ADR 0002: Localhost HTTP Fallback for Local Testing

**Status**: Accepted  
**Date**: 2026-07-07  
**Context**: Support for local E2E testing without TLS  

---

## Context

When testing identity services locally, protocols such as DID Configuration and OIDC specifically require the use of HTTPS for security reasons. For example, `DomainLinkageConfiguration::from_json_value` strictly mandates that fetched documents conform to these secure protocols. 

However, during local development and automated E2E testing, running a full TLS server stack and managing certificates is burdensome. We needed a simple way to boot the application locally on HTTP (`http://localhost`) and still have tests run without failing due to strict HTTPS checks.

---

## Decision

We introduced an `allow-localhost` feature flag that disables strict HTTPS checks for `localhost` URLs. 
Specifically, in `fetch_linked_dids`, when the `allow-localhost` feature is active, we attempt to fetch the domain linkage configuration via HTTP. Since `DomainLinkageConfiguration::from_json_value` strictly requires HTTPS, this fetch inevitably fails. We gracefully catch this error and default to returning no linked DIDs `(vec![], false)`, rather than causing the entire connection flow to panic or fail.

---

## Rationale

This approach provides a quick and non-intrusive workaround to allow local development and tests to proceed without needing a full TLS setup.

## Future Work

This is recognized as a non-ideal solution because it essentially disables domain linkage fetching in the local testing environment. In the future, we should implement a proper local HTTPS solution using a crate like `rcgen` to dynamically generate self-signed certificates during test initialization. This would allow the test harness to run on `https://localhost` natively, enabling us to test the entire domain linkage flow securely and without skipping validation logic.
