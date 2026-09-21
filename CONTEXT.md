# AI Context & Code Navigation

This guide helps AI agents and developers navigate the ssi-agent codebase by mapping domain concepts to implementation patterns and code locations.

## Architecture Overview

UniCore uses **domain-driven design (DDD)** with the **CQRS-ES pattern** (Command Query Responsibility Segregation + Event Sourcing):

- **Bounded Contexts**: Organized as separate crates (e.g., `agent_issuance`, `agent_holder`). Each context owns its domain model, commands, queries, and events.
- **CQRS**: Commands modify state, queries read state. Separated for clarity and independent optimization.
- **Event Sourcing**: All state changes are captured as immutable events. State is reconstructed by replaying events. Enables audit trails, temporal queries, and event-driven integration.
- **Aggregates**: Domain-driven design root entities that group related data and enforce business invariants.

## Bounded Context Mapping

Find where domain concepts live in code:

| Context               | Domain Focus                        | Primary Aggregates                    | Key Files                                               |
| --------------------- | ----------------------------------- | ------------------------------------- | ------------------------------------------------------- |
| `agent_issuance`      | Issuance Flow, Credential Offer     | `Offer`, `Credential`                 | `src/offer/aggregate.rs`, `src/credential/aggregate.rs` |
| `agent_holder`        | Holder, Wallet, Credential Storage  | `Holder`, `CredentialContainer`       | `src/holder/aggregate.rs`                               |
| `agent_verification`  | Verification, Presentation          | `Presentation`, `VerificationRequest` | `src/presentation/aggregate.rs`                         |
| `agent_authorization` | OAuth2, SIOP v2                     | `OAuth2AuthorizationRequest`          | `src/domain/oauth2_authorization_request/`              |
| `agent_identity`      | DID, Profile, Service               | `IdentityProfile`, `DIDDocument`      | `src/document/aggregate.rs`, `src/profile/aggregate.rs` |
| `agent_api_http`      | HTTP API, Handlers, Routing         | N/A (presentation layer)              | `src/handlers.rs`, `src/v0/`                            |
| `shared-kernel`       | CQRS Patterns, Dependency Injection | N/A (infrastructure)                  | `src/application_service.rs`, `src/command_handler.rs`  |

## Key Architectural Patterns

### Events

Events represent domain facts (e.g., `CredentialIssued`, `PresentationVerified`). Named in past tense. Each event is immutable and belongs to an aggregate.

**Location**: `src/domain/*/events.rs` or inline in aggregate files.

### Commands

Commands request a state change (e.g., `IssueCredential`, `VerifyPresentation`). Handled by an aggregate which either succeeds or fails atomically.

**Pattern**: Agent receives command → applies to aggregate → emits events → persists.

### Queries

Queries read state via projections (read models). CQRS separates reads from writes for independent optimization.

**Location**: Usually in `src/queries/` or service layer files.

### Aggregate Pattern

Aggregates group related entities under a root entity. The root enforces business rules and only the root is directly mutable from outside.

**Example**: `Offer` aggregate (in `agent_issuance`) contains offer metadata, credential templates, and issue state.

## Persistence Layer

- **Write Model**: Event store (PostgreSQL via `cqrs-es` crate)
- **Read Model**: Projections that materialize events into queryable views
- Both live in `infrastructure/stores/` (Postgres adapter)

## Cross-Context Integration

Contexts communicate via:

1. **HTTP (API boundary)** — External consumers call REST endpoints
2. **Events** — Published via event publisher (HTTP or NATS)
3. **Queries** — Read-only access to specific data

No direct database access between contexts; all coupling is intentional and explicit.

## When to Update This Guide

Update when:

- Adding or renaming a bounded context
- Introducing new architectural patterns
- Clarifying how concepts map to code
- Adding new aggregates or command types

---

**Last updated**: 2026-06-17
