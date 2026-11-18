# ADR 0012: In-Memory Views Rebuilt from MongoDB Events

**Status**: Accepted  
**Date**: 2026-11-18  
**Context**: Read models (views) of the MongoDB event store

---

## Context

Every aggregate type has two views: one per aggregate, and one listing all aggregates of the type
(`all_*`). With MongoDB, both were stored as documents next to the events. A list view is a single
document: each change of one item rewrote the whole document, its size grew with the number of items
up to MongoDB's document size limit, and concurrent commands could overwrite each other's changes to
it (an update losing a version conflict on a view is dropped).

The event store is the source of truth; views can be derived from it at any time. Since the
event-store writer lease (`docs/deployment/writer-lease.md`), one process writes to an event store at
a time, so the views of that process see every event.

## Decision

- With MongoDB, views are held in memory only. They are rebuilt from the stored events at startup,
  before any command runs, and updated in place by the committed events afterwards.
- The rebuild reads the `events` collection sorted by aggregate type and `_id`, the insertion order
  within an aggregate type, which is the order of the list views. It applies events to the two views
  of their aggregate type directly: no other query runs, so nothing is published and no command is
  executed again. Stored event metadata is passed along, as for committed events.
- Events of aggregate types without views here (a feature switched off, an extension's own
  aggregates) are skipped and counted. An event that cannot be applied is logged and counted; event
  verification reports it too and keeps the application unready.
- A view is updated in place under a write lock, so concurrent changes of a list view are all kept.
- PostgreSQL keeps persisted views: its `events` table has no insertion order to rebuild list views
  in.
- The in-memory event store uses the same in-memory views.

## Consequences

- Writes to list views no longer grow with the number of items, and list views have no size limit.
- Memory grows with the data of all views: about the size of the view collections in MongoDB
  (`credential`, `all_credentials`, `offer`, ...), times the overhead of the in-memory representation.
- Startup time grows with the number of stored events. The HTTP port is bound after the rebuild.
- The view collections in MongoDB are no longer written. A version that reads them sees views as of
  the upgrade; going back to it needs them rebuilt first.
- Views reading the time of an event take it from the `timestamp` metadata of the command
  (`CatalogView::modified_at`). Events stored before that metadata existed get the time of the
  rebuild.
