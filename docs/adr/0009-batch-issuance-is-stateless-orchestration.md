# ADR 0009: Batch Issuance Is Stateless Orchestration, Not an Aggregate

**Status**: Accepted  
**Date**: 2026-10-10  
**Context**: `POST /v0/verify-credentials-batch` and `POST /v0/create-credentials-batch`, which turn a
list of claims for one template into one credential and one offer per row.

---

## Context

An issuer wants to issue the same kind of credential to many holders at once: a teacher uploads a
CSV of grades, the frontend converts it into a request, and every row should become a credential
offered to that holder. A `Credential Offer` is redeemed by exactly one wallet for one credential
(`CreateCredentialResponse` issues a single credential), so a batch of N rows necessarily creates N
credentials and N offers.

The obvious design is a `Batch` aggregate (or a saga) that owns the rows, makes the run atomic and
can be inspected or retried later. The use case does not want that: after the run, the issuer works
with the general list of credentials. Nobody looks at "the batch" again.

---

## Decision

A batch is an application-level orchestration in the HTTP layer over the existing `Credential` and
`Offer` aggregates. There is no batch aggregate, no batch event, no stored batch state.

- **Rows are independent.** Each row is validated, created and offered on its own; a failing row
  does not hold back the others. The response lists every row's outcome by its `index` in the
  request: `201` when all rows were created, `207 Multi-Status` when some failed, and a
  `422 issuance#batch-failed` problem carrying the same per-row list when none could be created.
- **Partial rows are reported, not rolled back.** Creating a row is several events across two
  aggregates (credential created, offer created, credential added, offer sent). A row that fails
  part-way reports the IDs it did create so the caller can revoke or re-offer them. Recovery is
  "resubmit the failed rows"; there are no idempotency keys and no server-side retry.
- **Every row creates a new offer.** Offer and credential IDs are server-generated; a batch never
  adds to an existing offer, because an offer only ever issues one credential.
- **Verification is a query.** The verify endpoint runs the same validation and the same
  authorization pre-checks as the create endpoint but dispatches no command, so it leaves no events.
  It always answers `200` with per-row results; finding invalid rows is a successful verification.
- **Rows are claims, not envelopes.** A row carries the claims the template schema describes; the
  server wraps them for the template's format. Constants fixed by the schema are not filled in by
  the server: a row must satisfy the schema as written.

---

## Considered Options

- **A `Batch` aggregate.** Gives an audit record and a natural retry handle, but introduces state
  nobody reads, and still cannot make N credential/offer writes atomic without a saga on top.
- **All-or-nothing execution.** Rejected by the product owner: one bad row must not stop the
  class from getting their badges, and events cannot be undone anyway, so "nothing" is not achievable
  once execution has started.
- **Client-supplied offer IDs for replay safety.** Rejected: a CSV frontend has no natural IDs, and
  "create or add to existing offer" semantics would silently attach a second, unreachable credential
  to an offer on retry.

---

## Consequences

- The per-row response is the only record of a run. The frontend must act on it (show created rows,
  resend failed ones); it cannot ask the server about a past batch.
- An orphaned credential (created, never offered) can exist after an infrastructure failure. It is
  visible in the credential list as never issued, and the row that produced it names its ID.
- Email delivery is "requested", never "delivered": sending is an event consumed by an external
  publisher, so the response can only state that the send command was accepted.
