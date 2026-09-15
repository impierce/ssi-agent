# ADR 0006: Rebuild MongoDB Projections in Memory

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

MongoDB gains an opt-in `event_store.views: in_memory` mode. In that mode only events are persisted;
typed projections are held behind in-process read/write locks and rebuilt by streaming events at
startup. Replay updates view structs directly and never passes historical events through the CQRS
query pipeline, so it cannot republish events or execute commands.

One active application writer per MongoDB database is enforced with a renewable lease. Every event
append checks the lease owner, a monotonically increasing fencing token, and the server-time expiry
inside the event transaction. Losing the lease makes the process unready and triggers graceful
shutdown. Persisted-view mode retains its existing multi-replica behavior.

The setting defaults to `persisted` for an opt-in release. Postgres continues to use persisted
views.

## Constraints

Replay reconstructs `EventEnvelope` metadata as empty because no current view reads event metadata.
There are no event upcasters today, so stored payloads are deserialized directly into the current
event type. A future metadata-dependent view or upcaster must be added to both the command and
startup replay paths before it is deployed.

MongoDB in-memory mode requires transactions and therefore a replica set or sharded cluster.
Startup fails before initialization if events are incompatible, sequences contain gaps, or a view
cannot be rebuilt.

Global insertion order is derived from MongoDB ObjectId order. Replay buffers events when a legacy
multi-writer history contains per-aggregate ObjectIds out of sequence, then applies each aggregate
in sequence order. ObjectIds do not provide a durable incremental checkpoint, so tail catch-up is
outside this decision.

## Consequences

Projection writes are constant-time and no longer rewrite a growing BSON document. Projection
memory and startup time grow with the event stream and projected data. The API cannot scale by
running multiple in-memory writers; deployments needing multiple replicas must retain persisted
views until a durable synchronized projection consumer exists.

The process binds probe endpoints before state construction. Readiness stays false during lease
waiting and replay. Graceful SIGTERM handling drains requests and releases the lease.

Persisted views are not updated in in-memory mode. They remain useful for rollback during the
opt-in release but must be rebuilt before switching back after new events have been written.

This facility enables the dedicated `PublicTemplatesView` proposed in ADR 0004 without exposing an
event repository through the domain state or replaying events through publishers.
