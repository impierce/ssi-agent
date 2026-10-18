# ADR 0007: Hide Soft-Deleted Views Behind Application Queries

**Status**: Accepted
**Date**: 2026-10-05
**Context**: Reading templates, catalogs, connections and services, which are deleted by setting a flag on their views

---

## Context

Deleting a template, catalog, public offer, connection or service does not remove its view. The
view stays in the repository with a deletion flag (`status == Deleted`, `deleted` or `is_deleted`).

The view repository can't hide these views itself. `GenericQuery` and `ListAllQuery` load views
through the same repository when they apply events. If the repository returned `None` for a deleted
view, the next event for that entity would rebuild its view from `Default`.

Until now, every HTTP handler filtered deleted views itself. One reader forgot: the catalog's
template check counted deleted templates as existing, so a deleted template could be added to a
catalog.

---

## Decision

### Single-entity reads

Views implement `shared_kernel::view_repository::SoftDeletable`. Reads that should not see deleted
entities go through `shared_kernel::view_repository::load_by_id`, which returns `None` for a
soft-deleted view.

`agent_library`, `agent_identity` and the other modules that have no `ApplicationContext` yet each
get a `queries` module (`agent_library::queries`, `agent_identity::queries`). These queries run the
usual authorization check and load through `load_by_id`, via
`agent_shared::handlers::live_query_handler`. Inbound adapters use these queries instead of reading
the view repositories in the module's state directly.

### List views

How deleted entities are kept out of a list depends on whether deletion is terminal:

- **Terminal deletion** (templates, catalogs, public offers, connections): the list projection drops
  the entity when it is deleted. No later event can bring it back, so the projection never needs it
  again.
- **Reversible deletion** (services): the list projection keeps deleted entities and the list query
  filters them out. A service with no presentations or origins left is deleted, and it is revived
  when one is added again. Its view is updated incrementally, so dropping it from the list would
  rebuild a revived service from `Default`.

### Intentional readers of deleted views

Some readers have to see deleted views and keep reading the repository directly:

- `can_resolve_public_offer` and `TokenIssuanceService::can_redeem_offer`. Without a public-offer
  record, an offer counts as a normal, redeemable offer, so hiding deleted public offers would make
  them redeemable again.
- Internal service maintenance (`agent_identity::service::lifecycle`, `/.well-known/did-configuration.json`).

---

## Consequences

- New inbound adapters get deletion handling for free by using the module's queries.
- Two list strategies exist. Each module's `queries` documents which one applies.
- Dropping deleted templates from `AllTemplatesView` changes what is stored. We don't migrate stored
  list views and assume a clean database.
- When these modules move to `ApplicationContext`, their `queries` modules become its query handlers.
