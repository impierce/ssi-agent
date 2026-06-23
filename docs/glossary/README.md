# Glossary

This glossary is organized into two complementary documents:

## 📚 [Ubiquitous Language](./ubiquitous-language.md)

**Audience**: Domain experts, product stakeholders, API consumers, external developers

The shared language of UniCore. Pure domain concepts with no implementation details. This file is **publishable to external documentation** as-is.

Contains:

- **Core Identity Concepts**: DID, Credential, Holder, Issuer, Verifier, Wallet, Claim, Proof, Signature
- **OpenID4 Protocols**: OpenID4VCI, OpenID4VP, SIOP v2, Credential Offer, Pre-authorized Code, Challenge
- **Credential Lifecycle**: Issuance Flow, Presentation Flow, Verification, Selective Disclosure, Credential Status

### Using the Ubiquitous Language

When reading or writing about UniCore:

1. Use terms from this glossary for consistency
2. Challenge fuzzy language — if you're using multiple terms for the same concept, align on one
3. Reference this glossary when onboarding new stakeholders

---

## 🔧 [AI Context & Code Navigation](./CONTEXT.md)

**Audience**: AI agents, developers, architects

How domain concepts map to code. Explains architecture patterns (DDD, CQRS-ES, Aggregates, Events) and guides navigation from domain terms to implementation.

Contains:

- **Architecture Overview**: DDD, CQRS-ES, Aggregates, Events, Commands, Queries
- **Bounded Context Mapping**: Which contexts implement which domain concepts
- **Persistence Layer**: Write model (Event Store), Read model (Projections)
- **Cross-Context Integration**: How bounded contexts communicate

### Using AI Context

When navigating the codebase:

1. Start with a domain concept from [Ubiquitous Language](./ubiquitous-language.md)
2. Find where it lives using the [Bounded Context Mapping](./CONTEXT.md#bounded-context-mapping)
3. Understand the architectural pattern (Aggregate, Event, Command) from [Key Architectural Patterns](./CONTEXT.md#key-architectural-patterns)
4. Locate the specific code files

---

## Adding New Terms

Follow this process:

### 1. New Domain Concept

Add to [ubiquitous-language.md](./ubiquitous-language.md):

- Clear definition in plain language
- Related concepts
- No code references

### 2. New Implementation Pattern

Update [CONTEXT.md](./CONTEXT.md):

- Describe the architectural pattern or aggregate
- Map to relevant codebase locations
- Explain how it relates to domain concepts

### 3. New Bounded Context

Update [AI Context Mapping table](./CONTEXT.md#bounded-context-mapping):

- Add context name, domain focus, primary aggregates
- List key file locations

---

**Last updated**: 2026-06-17
