# ADR 0010: Stream-Based Event Publishers, Direct Integration Event Projection, and CloudEvents Standard

## Status
Accepted

## Context
To support standardized event streaming with caller provenance metadata on `CloudEvent`s, this refactoring addresses architectural debt and circular dependencies in event publishing:
1. **Monolithic `EventPublisher` Trait**: Previously, `agent_store` defined a monolithic `EventPublisher` trait and `Partitions` with duplicate aggregate query logic.
2. **Delivery Divergence**: NATS and SSE emitted `CloudEvent`s, while HTTP webhooks dispatched raw domain event payloads without standard envelopes.
3. **Configuration Schema Bloat**: `struct Events` in `agent_shared/src/config/mod.rs` duplicated ~250 lines of aggregate event enums.
4. **Domain Event vs Integration Event Coupling**: Exposing internal CQRS aggregate events directly to external webhook and SSE subscribers coupled external systems to internal aggregate schemas and risked leaking transient or sensitive domain data.

## Decision
1. **Deprecate Monolithic `EventPublisher`**:
   Remove `EventPublisher` and `Partitions` from `agent_store`. Query logic remains standard `cqrs_es::Query<A>`.
2. **Standardize on CNCF `CloudEvents` v1.0**:
   All outbound event forwarders (HTTP webhooks, NATS broker) and streaming APIs (SSE `/v0/events`) emit standardized `CloudEvent` payloads. Domain events never use the `CloudEvent` format and remain strictly `cqrs_es::EventEnvelope<A>`.
3. **Direct Integration Event Projection (`ProjectIntegrationEvent` & `EventProjector`)**:
   - Aggregates whose state changes are part of the Published Language implement `ProjectIntegrationEvent` (`fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent>`).
   - `EventBusHandle` implements `cqrs_es::Query<A> where A: ProjectIntegrationEvent`, projecting `&EventEnvelope<A>` directly into `Option<CloudEvent>` upon command commit, preserving deterministic event IDs (`{aggregate_type}:{aggregate_id}:{sequence}`), caller provenance (`callerid`, `callertype`), and timestamps while ignoring internal/unmapped events.
   - For persistent stores (`MongoEventSource`), `EventProjector::for_aggregate::<A>()` deserializes stored BSON documents into `EventEnvelope<A>` and invokes `A::project_integration_event(&envelope)` for Change Streams and `Last-Event-ID` catch-up queries.
4. **Configuration Simplification**:
   Replace duplicate aggregate event vectors with explicit integration event type lists:
   ```rust
   pub struct Events {
       pub types: Vec<String>,
   }
   ```
   Publishers dispatch only events whose `type` is explicitly listed in `events.types`.

## Consequences

### Positive
- **Boundary Decoupling**: Internal aggregate refactoring never breaks external integrations.
- **Contract Stability**: External subscribers depend on a published, versioned integration schema.
- **Data Minimization & Privacy**: PII and internal transient data are not broadcast over the integration bus.
- **Unified Delivery**: All delivery channels (SSE, Webhooks, NATS) consume the exact same `EventBusHandle` and CloudEvents contract.

### Negative / Breaking Changes
- **HTTP Webhook Payload Format**: Webhook targets now receive CNCF CloudEvents envelopes (`id`, `source`, `type`, `time`, `callerid`, `callertype`, `data`) instead of raw aggregate event payloads.
- **Configuration Migration**: `events: { credential: [...], ... }` is replaced by `events: { types: [...] }`.
