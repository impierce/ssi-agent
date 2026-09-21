# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — bounded-context → crate mapping, aggregates, and the key files for each context.
- **`docs/glossary/ubiquitous-language.md`** — the domain-pure vocabulary (DID, Credential, Holder, Issuer, Verifier, Issuance Flow, …), free of implementation detail.
- **`docs/adr/`** — read the ADRs that touch the area you're about to work in.

This is a **single-context** repo: one `CONTEXT.md` at the root, one `docs/adr/`. There is no `CONTEXT-MAP.md`; the per-crate bounded contexts (`agent_issuance`, `agent_holder`, `agent_verification`, `agent_authorization`, `agent_identity`, …) are mapped from within the root `CONTEXT.md`.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The producer skill (`/grill-with-docs`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
/
├── CONTEXT.md                          ← bounded contexts, aggregates, code navigation
├── docs/
│   ├── adr/
│   │   ├── 0001-schema-properties-attributes-as-separate-field.md
│   │   └── …
│   └── glossary/
│       ├── README.md
│       └── ubiquitous-language.md      ← domain-pure terms, publishable as-is
└── agent_*/                            ← one crate per bounded context
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md` and `docs/glossary/ubiquitous-language.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/grill-with-docs`).

Keep the split intact: `ubiquitous-language.md` stays implementation-free; code locations and patterns belong in `CONTEXT.md`.

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0003 (hermetic test architecture and config decoupling) — but worth reopening because…_
