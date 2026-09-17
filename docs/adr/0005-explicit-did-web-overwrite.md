# ADR 0005: Require Explicit Authorization to Overwrite the Deployment `did:web`

**Status**: Accepted  
**Date**: 2026-09-09  
**Context**: Stable deployment identity and runtime Domain Linkage management

---

## Context

UniCore derives a deployment `did:web` from the origin of `public_url` when it first creates the
DID document. That DID is then persisted and used as an issuer or controller identifier by
credentials, status information, Domain Linkage credentials, and external trust relationships.

The public URL is deployment configuration. It can change because of a proxy correction, domain
migration, environment replacement, or an accidental configuration edit. Recomputing and replacing
the persisted DID on every startup would turn any such change into an identity change. Keeping the
same signing keys does not preserve the identity: `did:web:old.example` and
`did:web:new.example` are different identifiers, and credentials naming the former still resolve
the DID document at the old origin.

At the same time, deployments need a supported way to move intentionally to a new origin. Making a
persisted DID completely immutable would force operators to wipe state or modify the event store,
both of which bypass the application's audit and consistency boundaries.

## Decision

The persisted deployment `did:web` is the source of truth after its first creation.

On startup, UniCore derives the DID implied by `public_url` and compares it with the persisted DID.
If they differ, startup fails before changing any DID document. The error identifies the persisted
DID and the configured origin.

An overwrite is permitted only when the optional `overwrite_previous_did_web` setting exactly
matches the persisted DID. The destination is not accepted as free-form input; it is derived from
`public_url`. The corresponding environment variable is
`UNICORE__OVERWRITE_PREVIOUS_DID_WEB`.

The setting deliberately uses "overwrite" rather than migration or rotation language. Changing the
identifier replaces the deployment's current identity and does not update credentials that refer to
the previous DID.

The overwrite command carries the expected previous DID and the document aggregate verifies that
it still matches the current document. A successful change emits `DocumentDidWebOverwritten`,
including the previous DID and the new document. Existing verification methods and services are
remapped to the new DID, and active Domain Linkage credentials are renewed for the new origin.

The authorization setting is a one-time operational guard. Operators remove it after the successful
startup. If it is accidentally left in place, it cannot authorize a later migration because its DID
will no longer match the persisted DID.

This decision applies only to the deployment `did:web`. Configuration-driven initialization of
other DID methods is unchanged.

## Rationale

Requiring an exact previous DID makes the operator acknowledge which identity is being replaced.
It also protects against stale deployment automation: a generic boolean such as
`overwrite_previous_did_web: true` could authorize an unrelated future domain change, whereas an
already-consumed DID value cannot.

Deriving the destination from `public_url` keeps the DID, hosted DID document, and Domain Linkage
origin aligned. Recording the transition as an event preserves the event-sourced audit trail and
lets projections reconstruct the same current document.

Keys are retained because key rotation and identifier migration are separate operations. Retaining
them avoids an unnecessary second trust change, but it does not make the old and new DIDs equivalent.

## Consequences

- Routine public URL drift prevents startup instead of silently re-identifying the deployment.
- A planned origin migration requires a temporary configuration change and produces an explicit
  audit event and warning log.
- Existing credentials are not migrated. They continue to name the old DID.
- Operators must keep the old DID document resolvable at its old origin for as long as credentials
  issued under that DID must remain verifiable.
- The application cannot guarantee old-origin availability after a migration because resolution of
  the old `did:web` is controlled by the old origin's hosting and DNS.

## Alternatives considered

### Automatically follow `public_url`

Rejected because a routine or accidental configuration change would silently create a different
identity and break verification of previously issued credentials.

### Never allow replacement

Rejected because legitimate domain migrations would require deleting persisted state or editing the
event store outside the domain model.

### Use a boolean force flag

Rejected because a stale flag could authorize later, unintended migrations. Naming the expected old
DID makes the authorization specific and self-expiring.

### Keep the old DID as another active aggregate

Not selected for this migration mechanism. Retaining an old document in the new deployment's event
store does not keep it resolvable: a `did:web` document is fetched from the origin encoded in that
DID. Continued verification therefore requires the operator to serve the old document from the old
origin, independently of which document the new deployment treats as current.
