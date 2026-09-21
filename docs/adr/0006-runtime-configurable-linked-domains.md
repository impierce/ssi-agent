# ADR 0006: Runtime-Configurable Linked Domains

**Status**: Accepted
**Date**: 2026-09-10
**Context**: Self-service linked-domain setup for multi-tenant SaaS deployments

---

## Context

A multi-tenant deployment needs its tenants to set up their own linked domains: enter a domain, see
DNS instructions for it, and verify the result. Tenants have no access to their UniCore instance's
environment variables or deployment pipeline, so this must work **entirely at runtime, with no
configuration edits and no redeploy**.

## The deployment identity and the linked domain are separate

Domain linkage proves "the controller of this DID also controls this domain". Nothing about the
mechanism requires the DID being proven and the domain being claimed to be the same thing:
`did:iota` links a domain with no textual relationship between its identifier and that origin.

So the deployment's own identity — the immutable name it is given at initial deployment — stays as
static as it already is, and linking a domain is not an identity migration. None of ADR 0005's
concerns apply: there is no DID to overwrite, no persisted-DID confirmation, and no conflict to
reject. A tenant supplies domains; UniCore publishes credentials claiming them.

## Multiple domains, per the specification

The [DID Configuration specification](https://identity.foundation/well-known-did-configuration/resources/did-configuration/#linked-domains)
treats a single origin as the narrow case rather than the general one:

- A `LinkedDomains` `serviceEndpoint` is *either* a bare origin string *or* an object with an
  `origins` array of one or more origins.
- Each Domain Linkage Credential claims **exactly one** origin, so linking *N* domains with *M*
  signing keys means *N × M* credentials.
- `JwtDomainLinkageValidator::validate_linkage` is **at-least-one, not all-must-match**
  (`identity_credential/src/domain_linkage/domain_linkage_validator.rs:62-84`): validated against
  origin A, credentials for B are error-partitioned and ignored, and the call still succeeds. One
  shared `did-configuration.json` carrying every origin's credentials is therefore spec-conformant,
  and full coverage is asserted by looping the validator once per origin.

## Decisions

- **Three endpoints**: `POST /v0/add-linked-domains`, `POST /v0/remove-linked-domains`,
  `GET /v0/verify-linked-domains`. Both mutations take an `origins` array.
- **The `serviceEndpoint` uses whichever of the specification's two shapes fits**: a bare origin
  string for a single linked domain, the `origins` array for several.
- **Idempotent, uncapped, conflict-free.** Linking an already-linked domain is a no-op; so is
  unlinking one that is not linked. There is no `409` anywhere, and no limit on how many domains may
  be linked. Unlinking the last domain withdraws the service entirely.
- **Unlinking never re-signs.** It filters the published credentials by the origin each one claims,
  keeping the remaining domains' credentials exactly as issued. That also means a domain can be
  withdrawn after the signing keys have become unavailable, rather than stranding a tenant who
  disabled `did:web`.
- **Origins are canonicalized on the way in** (scheme, host, non-default port; path discarded), so
  the stored set, the credentials' claims and the published endpoint always agree, and so the set is
  order-independent and byte-stable.
- **Verification checks both DNS and linkage, per origin.** For each origin UniCore fetches that
  origin's `/.well-known/did-configuration.json` and validates it, and separately resolves the
  origin's `CNAME` chain with caching disabled to report whether it points at `public_url`'s host.
  The per-origin `valid` reflects the **linkage** check; DNS is a diagnostic that explains a failure.
  This is deliberate: an apex domain cannot have a `CNAME` record, so gating on DNS would report a
  correctly-served apex domain as broken.
- **`public_url` is untouched.** It remains static, config-derived and never mutated at runtime, and
  it defaults to `application_url`. It denotes only the deployment's own address — the place a linked
  domain's DNS is pointed at — never the origin a credential claims.
- **The `.well-known/did-configuration.json` handler stays Host-agnostic**, serving one document
  holding every origin's credentials. That is spec-conformant (see above) and safe because this ADR
  assumes one deployment per tenant, so every linked origin belongs to the same tenant. Serving
  per-Host subsets is the change to make if that assumption ever stops holding.

## Consequences

Domain linkage is managed exclusively through the API; there is no configuration setting that links a
domain. A tenant-facing flow is three calls: `add-linked-domains` with the domain, a display of the
`CNAME` target, then `verify-linked-domains`, whose `dns.points_here` says specifically whether DNS
is the thing still missing.

The deployment DID remains governed by ADR 0005: derived from `public_url` at first creation, and
changed only by the explicit startup overwrite described there.
