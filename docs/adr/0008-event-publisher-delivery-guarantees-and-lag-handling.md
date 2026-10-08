# ADR 0008: Event Publisher Delivery Guarantees and Lag Handling

**Status**: Accepted  
**Date**: 2026-10-06  
**Context**: Outbound event publishers (`HttpEventPublisher`, `NatsEventPublisher`), in-process buffering, and delivery semantics under load.

---

## Context

Previously, event publishers implemented `cqrs_es::Query<A>` directly:
- When an aggregate committed, `query.dispatch` was invoked inline. The NATS publisher awaited the broker round-trip there; the HTTP publisher already sent its request from a detached task.
- Delivery was attempted for every committed event but never guaranteed: a failed request or publish was only logged. The inline NATS publish still coupled aggregate write transactions to broker latency and availability.

To decouple aggregate commits from network I/O, UniCore moved to an in-process pub/sub `EventBusHandle` backed by a `tokio::sync::broadcast::channel(1024)`:
- Outbound publishers consume events asynchronously.
- The write path never blocks on subscribers.
- However, bounded broadcast channels are lossy: if a subscriber falls more than 1024 events behind, Tokio yields `EventBusError::Lagged(n)`. Previously, publishers discarded this error silently via `if let Ok`.

---

## Decision

We address event delivery in two phases:

### 1. Immediate: Explicit Warning Logs
Neither publisher silently swallows lagged events. `HttpEventPublisher` and `NatsEventPublisher` explicitly match `EventBusError::Lagged(count)` and emit a `tracing::warn!` with the number of dropped events.

### 2. Long-Term: Transactional Outbox / Durable Streams
For mission-critical integration events requiring strict at-least-once delivery (e.g. transactional emails or audit records), replace in-memory broadcast dispatch with:
- A **Transactional Outbox** table written atomically with aggregate commits and drained by a background worker, or
- A durable streaming broker with persistent consumer offsets (e.g. NATS JetStream or Kafka).

---

## Consequences

- **Visibility**: Operators are immediately alerted if downstream backpressure causes event loss.
- **Write Performance**: The core write path remains fast and isolated from external network latency.
- **Trade-off**: Until a durable outbox or stream is implemented, sustained burst traffic exceeding the buffer remains best-effort (at-most-once delivery).
