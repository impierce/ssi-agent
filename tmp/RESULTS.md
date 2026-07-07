# Review: `feat/upcast-events` (HEAD `61a1eca9`) vs `beta` (`8434e8ea`)

Reviewed 2026-07-07. Merge base is `b20573b9`; the branch adds **5 commits** on top of it, and
beta adds exactly **1 commit** the branch doesn't have (`8434e8ea` "feat: introduce catalogs").
A raw `git diff 8434e8e..HEAD` therefore shows the catalog files as "deleted" — that is an
artifact of the branch predating them, not an actual removal.

**Verdict: the branch matches the desired outcome** (plan of record in
`EVENT_BREAKING_CHANGES.md`): persisted event stores survive breaking event-schema changes via
versioned events + upcasters, guarded by tests and a startup replay validation surfaced through
`/readyz`. One follow-up is required when merging beta in (see "Gap vs beta" below).

## What the branch changes, by theme

### 1. Event versioning convention (all 19 event enums)
- Every domain event enum now carries an explicit integer schema version (all still `"1"`) with
  a comment pointing at the convention doc, plus an (empty) `upcasters()` registry function next
  to the enum. `docs/event-versioning.md` defines when to bump, when not to (additive changes),
  and how to write an upcaster.
- Verified: 19/19 enums have the scaffold, 19/19 still return version `"1"`.

### 2. Upcaster plumbing through both store abstractions
- `agent_store` (the path the application actually runs on): `commands_and_queries` now takes
  the aggregate's upcasters and applies them in the event store for Postgres and MongoDB.
  Postgres had to be built manually instead of via the `postgres_es` convenience constructor
  (which can't accept upcasters) — behavior/table naming preserved. The in-memory backend
  accepts and ignores them (its store never serializes events, documented inline).
- `shared-kernel` / `infrastructure/stores/mongodb` (the newer, parallel store path):
  `CommandHandlerFactory::create_handler` gained the same `upcasters` parameter; the Mongo store
  wires them in identically. The two paths still coexist by design; both got the same treatment.
- Verified: all 19 aggregate call sites pass their `upcasters()`.

### 3. Wire-format regression tests (all 78 event variants)
- Every event enum has a test module asserting each variant serializes to a checked-in "golden"
  JSON literal and round-trips losslessly. Any accidental rename/shape drift now fails CI with a
  pointer to the versioning doc instead of silently corrupting compatibility with persisted data.
- Minor cosmetic inconsistency: 10 modules are named `wire_format_tests`, 9 are named
  `event_tests` (different sub-agents); content and coverage are equivalent.

### 4. Startup replay validation + `/readyz` readiness gate
- On boot (before serving traffic), the app streams **every persisted event of every aggregate**
  through the upcaster + deserialization pipeline. Success → `/readyz` returns `200 ready`.
  Failure → the process **stays up** (`/healthz` unchanged), logs loudly, and `/readyz` returns
  `503` with a descriptive reason (aggregate type, events validated before failure, underlying
  error) — so an orchestrator holds back the bad revision instead of crash-looping it.
- Config flag `event_replay_validation` (default **on**, `UNICORE__EVENT_REPLAY_VALIDATION=false`
  to skip). In-memory backend trivially reports 0 events.
- Existing `router()` callers are unaffected (they get an always-ready handle).

### 5. End-to-end legacy-event proof (Chunk 7)
- Unit level: a hand-built version-`"1"` event with an old payload shape fails to deserialize
  as-is, and parses into the current enum after a demo `"1"`→`"2"` upcaster (on a real enum,
  `NonceEvent`). Confirms cqrs-es's `SemanticVersionEventUpcaster` works with the bare-integer
  convention (note: it stamps `"2.0.0"`; harmless, documented).
- Integration level: a legacy event seeded into a persisted in-memory repository makes the real
  replay-validation routine fail with the descriptive error, and pass once the upcaster is
  registered.
- Boot level: the validation outcome (both success and failure) is asserted through the actual
  `/readyz` HTTP response.

### 6. Unrelated but included
- `b32ea054`: explicit SIGTERM/SIGINT handling (the agent runs as PID 1 in containers; without
  it, `docker stop`/Kubernetes had to escalate to SIGKILL). Re-validated: sub-second shutdown.
- `794cd55a`: rust-analyzer configured with all features (editor-only).
- The `/readyz` commit also carries `cargo fmt` fixes for a few earlier files committed with
  formatting violations (CI enforces `cargo fmt --all -- --check`).

## Verification status

`cargo test --workspace`: 446 passed, 0 failed. Clippy and rustfmt clean. Live boot check
against the in-memory store: startup validation logged success, `/readyz` → `200 ready`,
`/healthz` → `200`, SIGTERM shutdown immediate.

## Gap vs beta: the new `Catalog` aggregate ⚠️

Beta's `8434e8ea` introduces a 20th aggregate (`Catalog` in `agent_library`, 7 event variants)
that this branch predates. When merging beta into this branch:

1. `agent_store`'s catalog `commands_and_queries` call uses the old 2-argument signature — the
   merge **will not compile** until `agent_library::catalog::event::upcasters()` is passed
   (good: can't be missed).
2. `catalog/event.rs` needs the standard treatment: version comment on `event_version()`, the
   `upcasters()` scaffold, and wire-format golden tests for its variants.
3. **Silent gap — must be done by hand:** `validate_all_events` in `agent_store/src/lib.rs` is a
   manually maintained list; `Catalog` must be added to the sweep or its events are excluded
   from startup replay validation *without any compiler error*. Consider a follow-up to make
   that list harder to forget (e.g. a macro or a test comparing it against the registered
   aggregates).
