# ADR 0004: Derive the Public Templates View at Query Time

**Status**: Accepted  
**Date**: 2026-08-21  
**Context**: Read model backing the `GET /public/templates` endpoint  

---

## Context

The `GET /public/templates` endpoint exposes every template that is both `visibility: public` and
`status: published` to unauthenticated callers. This required deciding where the read model backing
that endpoint should live.

Three options were considered:

1. **Derive at query time** from the existing `all_templates` view.
2. **A dedicated in-memory view**, held in a `MemRepository` and reconstructed on every application
   startup by replaying the `Template` event stream.
3. **A dedicated persisted view**, written to its own MongoDB collection / Postgres table via the
   normal `ListAllQuery` path.

Option 3 was ruled out early: it means an additional collection to provision and operate, and it
needs a backfill for templates that were already public before the endpoint was deployed.

Option 2 was explored in some depth. The underlying primitive exists — cqrs-es 0.5 defines
`PersistedEventRepository::stream_all_events::<A>()`, and both `mongo-es` and `postgres-es`
implement it. It is not reachable from where it would be needed, however: the event repository is
owned privately inside `AggregateHandler.cqrs`. Exposing it would mean changing the
`CqrsComponentBuilder` trait and all three of its implementations (`in_memory`, `mongodb`,
`postgres`), then `library_state()` in `agent_store`, and finally adding an await-before-serve
startup phase in `agent_application::state()`.

It would also be the first event replay in the codebase. Every view today is built incrementally by
`ListAllQuery` as events are dispatched; nothing is ever reconstructed from the log. The closest
precedent, `load_raw_events()`, reads the entire events collection at startup — but for event
*verification*, not for building a view.

---

## Decision

We derive the public templates list at query time from the existing `all_templates` view.

The handler in `agent_api_http/src/public/templates.rs` loads that view through
`public_query_handler`, filters it to `visibility == Public && status == Published`, sorts by
`modified_at` descending, and maps the result to `PublicTemplateDto`. No new `View` implementation,
no new view repository, no new collection, and no new startup phase.

The exposed model is deliberately narrower than the internal `TemplateDto`. It omits both
identifiers (`id`, `sourceTemplateId`) and `holderAuthorization`; `status` and `visibility` are
omitted because every template in the response is by definition published and public, so both would
be constants. `modifiedAt` is retained: absent a template versioning system, it is the only signal a
reader has of how fresh a template is.

---

## Rationale

This is the option with no structural cost. It introduces exactly one projection — the one that
already exists — so there is no second read model that can drift from the aggregate, nothing extra
to provision, and no change to application startup. It also matches the shape of the sibling
handlers `get_templates` and `get_all_catalogs`, which filter the same way.

---

## Consequences

Each request to `GET /public/templates` loads and deserializes the entire `all_templates` document,
including private and draft templates that are then discarded. The per-request cost is therefore
O(all templates) rather than O(public templates).

This is not a new cost — `GET /v0/list-all-templates` already performs the same read — but this
decision does place that read on an unauthenticated path for the first time.

---

## Future Work

This should be revisited if `/public/templates` starts carrying meaningful traffic while the
template set grows, since the O(all templates) read per request is the pressure point.

The intended end state is option 2: a dedicated `PublicTemplatesView` that is pre-filtered and
pre-shaped, held in memory, and reconstructed by event replay at startup. That refactor is
deliberately deferred until in-memory views built via event replay exist as a general facility in
this codebase, rather than being introduced for a single endpoint. When that facility lands, the
handler should stop filtering `all_templates` and read the dedicated view instead.

A related open question is identity: the public model intentionally carries no identifier, which
makes the endpoint a snapshot feed rather than an addressable collection. A reader cannot ask
whether a template it previously imported has since changed. If re-synchronisation ever becomes a
use case, a stable opaque handle — not necessarily the internal aggregate id — would need to be
added.
