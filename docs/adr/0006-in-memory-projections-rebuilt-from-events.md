# ADR 0006: Rebuild Projections in Memory

**Status**: Accepted
**Date**: 2026-09-15
**Context**: Event-sourced read-model storage and list ordering

---

## Context

Every aggregate is projected into a single-aggregate view and an `all_*` view. Persisting an
`all_*` view as one MongoDB document makes each write proportional to the number of existing items
and eventually reaches MongoDB's document-size limit. The `HashMap` previously used by those views
also produced unstable list ordering after each serialization round trip.

## Decision

All `all_*` projections use `IndexMap`, and list endpoints expose newest-created items first by
reversing insertion order. Template endpoints retain their separate most-recently-modified order,
using parsed RFC 3339 timestamps and an identifier tie-breaker.

Only events are persisted. Typed projections are held behind in-process read/write locks and
rebuilt from the configured MongoDB or PostgreSQL event store at startup. Replay updates view
structs directly and never passes historical events through the CQRS query pipeline, so it cannot
republish events or execute commands.

One active application writer per event-store database is enforced with a database-backed lease.
Every event append checks the lease owner and a monotonically increasing fencing token inside the
event transaction. Losing the lease makes the process unready and triggers graceful shutdown.

Projection storage is not configurable. `event_store.type` chooses where the event log lives, not
where views live. If external projection stores are introduced later, they will be configured as
independent query-side adapters so different projections can target different technologies.

## Constraints

Replay reconstructs `EventEnvelope` metadata as empty because no current view reads event metadata.
There are no event upcasters today, so stored payloads are deserialized directly into the current
event type. A future metadata-dependent view or upcaster must be added to both the command and
startup replay paths before it is deployed.

MongoDB requires transactions and therefore a replica set or sharded cluster. PostgreSQL uses an
advisory-lock-backed lease and fences event appends transactionally. Startup fails before
initialization if events are incompatible, sequences contain gaps, or a view cannot be rebuilt.

Replay buffers events when legacy history is not read in per-aggregate sequence order, then applies
each aggregate in sequence order. The current event-store ordering does not provide a durable
incremental checkpoint, so tail catch-up is outside this decision.

## Consequences

Projection writes are constant-time and no longer rewrite growing database documents. Projection
memory and startup time grow with the event stream and projected data. The API cannot scale by
running multiple active writers until a durable synchronized projection consumer exists.

The process binds probe endpoints before state construction. Readiness stays false during lease
waiting and replay. Graceful SIGTERM handling drains requests and releases the lease.

Legacy persisted views are no longer updated. They can help compare a first rollout but must be
rebuilt before rolling back to an older version after new events have been written.

This facility enables the dedicated `PublicTemplatesView` proposed in ADR 0004 without exposing an
event repository through the domain state or replaying events through publishers.
