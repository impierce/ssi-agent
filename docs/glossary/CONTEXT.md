# AI Context Guide

This file provides context specifically for AI agents working on the ssi-agent codebase.

## Using the Glossary for AI Context

The glossary in this folder (`terms.md` and `README.md`) serves as canonical domain language reference. When working on code changes:

1. **Verify terminology** - Use the glossary to ensure you're using the correct domain terms
2. **Understand relationships** - Each term includes related concepts to help you understand connections
3. **Find code locations** - Terms reference which bounded context or component implements the concept

## Bounded Context Mapping

| Context               | Primary Concept                    | Key Files                                               |
| --------------------- | ---------------------------------- | ------------------------------------------------------- |
| `agent_issuance`      | Issuance Flow, Credential Offer    | `src/offer/aggregate.rs`, `src/credential/aggregate.rs` |
| `agent_holder`        | Holder, Wallet, Credential Storage | `src/credential/aggregate.rs`, `src/offer/aggregate.rs` |
| `agent_verification`  | Verification, Presentation         | `src/presentation/aggregate.rs`                         |
| `agent_authorization` | OAuth2, SIOP v2                    | `src/domain/oauth2_authorization_request/`              |
| `agent_identity`      | DID, Profile, Service              | `src/document/aggregate.rs`, `src/profile/aggregate.rs` |
| `agent_api_http`      | HTTP Handlers, API Routing         | `src/handlers.rs`, `src/v0/`                            |
| `shared-kernel`       | CQRS Patterns, DI                  | `src/application_service.rs`, `src/command_handler.rs`  |

## When to Update This Glossary

Update the glossary when:

- Adding a new bounded context
- Introducing new domain concepts
- Clarifying confusing terminology
- Linking new features to existing domain language

Keep the glossary synchronized with code changes that affect domain understanding.

---

**Last updated**: 2026-06-17
