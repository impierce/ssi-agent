# ADR 0007: Stream-Based Event Publishers, Dual-Bus Architecture, and CloudEvents Integration Standard

## Status
Accepted

## Context
Building on `feat/actor-in-event` (which adds caller metadata extensions to `CloudEvent`), this refactoring addresses architectural debt and circular dependencies in event publishing:
1. **Monolithic `EventPublisher` Trait**: Previously, `agent_store` defined a monolithic `EventPublisher` trait and `Partitions` with duplicate aggregate query logic.
2. **Delivery Divergence**: NATS and SSE emitted `CloudEvent`s, while HTTP webhooks dispatched raw domain event payloads without standard envelopes.
3. **Configuration Schema Bloat**: `struct Events` in `agent_shared/src/config/mod.rs` duplicated ~250 lines of aggregate event enums.
4. **Domain Event vs Integration Event Coupling**: Exposing internal CQRS aggregate events directly to external webhook and SSE subscribers coupled external systems to internal aggregate schemas and risked leaking transient or sensitive domain data.

## Decision
1. **Deprecate Monolithic `EventPublisher`**:
   Remove `EventPublisher` and `Partitions` from `agent_store`. Query logic remains standard `cqrs_es::Query<A>`.
2. **Standardize on CNCF `CloudEvents` v1.0.2**:
   All outbound event forwarders (HTTP webhooks, NATS broker) and streaming APIs (SSE `/v0/events`) emit standardized `CloudEvent` payloads.
3. **Dual-Bus Architectural Isolation**:
   - `domain_event_bus`: Internal CQRS event store bus that captures private domain events emitted by aggregate commits across all bounded contexts.
   - `integration_event_bus`: Public integration event bus that streams stable Published Language (PL) events (`tech.impierce.unicore.*`) to external consumers.
   - **Integration Projectors**: Background projector tasks (`start_core_integration_projector` and `start_ext_integration_projector`) translate internal domain events to public integration events, preserving caller provenance (`callerid`, `callertype`) and timestamps while dropping internal/unmapped events.
4. **Configuration Simplification**:
   Replace duplicate aggregate event vectors with pattern-based matching:
   ```rust
   pub struct Events {
       pub types: Vec<String>,
   }
   ```
   Filter patterns support exact matches, wildcards (e.g. `tech.impierce.unicore.issuance.*`), and legacy kebab-case suffixes.

## Consequences

### Positive
- **Boundary Decoupling**: Internal aggregate refactoring never breaks external integrations.
- **Contract Stability**: External subscribers depend on a published, versioned integration schema.
- **Data Minimization & Privacy**: PII and internal transient data are not broadcast over the integration bus.
- **Unified Delivery**: All delivery channels (SSE, Webhooks, NATS) consume the exact same `integration_event_bus` and CloudEvents contract.

### Negative / Breaking Changes
- **HTTP Webhook Payload Format**: Webhook targets now receive CNCF CloudEvents envelopes (`id`, `source`, `type`, `time`, `callerid`, `callertype`, `data`) instead of raw aggregate event payloads.
- **Configuration Migration**: `events: { credential: [...], ... }` is replaced by `events: { types: [...] }`.
