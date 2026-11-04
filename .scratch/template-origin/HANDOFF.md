# Handoff: Template Origin (#493)

Status: ready-for-agent

Branch: `feat/template-origin`, cut from `beta`. This repository is **squash-merge only**.

## Goal

Implement https://github.com/impierce/ssi-agent/issues/493. A template gets an optional **origin**
that records where it was imported from: `source`, `sourceTemplateId` and `digest`. A public digest
function lets callers detect two things: that the source changed since the import, and that the copy
was edited locally. The issue is the spec; this file adds the code map and the decisions it leaves
open.

## Decisions already made: don't re-ask

- The origin is **optional**. Templates without one behave exactly as today.
- The existing `source_template_id` keeps its meaning, a local copy of a local template, and is
  **not** reused for the origin.
- The origin is set on create and can be replaced later. Re-importing updates the same template in
  place: same id, new content, new origin digest.
- The origin is **never** part of the public template projection (`GET /public/templates`).
- `digest` = SHA-256 over the **RFC 8785 (JCS)** canonical JSON of the template's **public content**.
  The public content is the fields of the public projection without `modifiedAt`. It is change
  detection only, so there's no version ordering or history.
- Fetching or verifying a source's templates is the caller's business. This repository only stores
  and compares.

## Open choices: recommendations, decide and document

- **Digest encoding:** `sha256-` followed by lowercase hex. Pin it with test vectors, including key
  reordering, whitespace, Unicode, and numbers in `schema`.
- **Where the public content lives:** today the projection is `PublicTemplateDto` in
  `agent_api_http/src/public/templates.rs`. Define the public-content shape **in `agent_library`**,
  so the digest function and the API DTO share one definition and can't drift.
- **How the origin is replaced:** template updates are per-field commands today (`Update*` in
  `agent_library/src/template/command.rs`). Add a small `UpdateOrigin { template_id, origin: Option<…> }`
  command and event, rather than one atomic "replace everything" command. A caller re-importing
  updates the fields first and the origin last. If it stops halfway, the stale digest still shows the
  source as changed, so the state heals itself.
- **JCS implementation:** neither workspace has a crate for it yet. Pick a maintained one (e.g.
  `serde_jcs`) or implement RFC 8785 carefully. Check `deny.toml` before adding the crate.

## Code map

- **Aggregate:** `agent_library/src/template/aggregate.rs`
  - the state struct has `source_template_id` around line 193;
  - `CreateNewTemplate` is handled around lines 233 and 341;
  - events are applied around lines 639–656.
- **Other template files:** commands in `agent_library/src/template/command.rs`, events in
  `event.rs`, read models in `views/`, tests in `aggregate_tests.rs` (follow its style).
- **HTTP:** `agent_api_http/src/v0/templates/mod.rs` (internal template DTOs; the origin appears
  here) and `agent_api_http/src/public/templates.rs` (public projection; the origin must not appear).
- **Other `CreateNewTemplate` callers** need the new field: `agent_issuance/src/reissuance/service.rs`,
  `agent_api_http/src/v0/issuance/{public_offers,credentials,reissuance}.rs`,
  `agent_holder/src/offer/aggregate.rs`, `agent_issuance/tests/credential_configuration_projection.rs`.
- **Domain docs:**
  - add **Template Origin** to `docs/glossary/ubiquitous-language.md`, implementation-free, next to
    "Credential Template";
  - point to the code from `CONTEXT.md` if the template section lists key files;
  - an ADR only if the digest or public-content decision meets the bar in `docs/adr/`.

## Conventions

- Read `CLAUDE.md`/`AGENTS.md` first.
- **Commits:** conventional commits (commitlint). Leave `CHANGELOG.md` alone.
- **OpenAPI** is generated: `cargo test generate_openapi_spec`. Never edit `agent_api_http/openapi*.yaml`
  by hand. Follow the `operation_id` rules in `CLAUDE.md` for any new operation.
- **Compatibility:** stored events must keep deserializing, so give the new optional field
  `#[serde(default)]`. Other deployments may run this code.
- **Public repository:** keep wording generic, with no product, partner or customer names in code,
  docs, commits or the PR.
- **Commit and push only with the maintainer's explicit approval.** Stage, propose the messages and
  ask.
- **Before proposing:** `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test`.

## When it merges

A downstream repository pins this one as a submodule. Tell the maintainer the **squash-merged commit
on `beta`**, because pointers to the PR branch head dangle after the squash.

## Suggested skills

- `tdd`: digest test vectors first, then the aggregate behaviour.
- `review`: before proposing commits or marking the PR ready, against #493.
- `zoom-out`: if what belongs in `agent_library` versus `agent_api_http` gets unclear.
