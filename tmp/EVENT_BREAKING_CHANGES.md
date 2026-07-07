# Handling Breaking Changes on the Event Read Side

> **Status (2026-07-06, branch `feat/upcast-events`):**
>
> - **Part F (SIGTERM / fast shutdown): DONE** — implemented and validated in this repo, see
>   [Part F status](#part-f--shutdown-behaviour-the-delay-is-missing-signal-handling-not-graceful-shutdown).
> - **Parts A–D: Chunks 1–6 DONE** (integer versioning + scaffolds + docs, shared-kernel factory +
>   in-memory persisted repo, infra-mongo `validate_events`, agent_store upcaster wiring,
>   round-trip/golden tests for all 78 event variants, `/readyz` + startup replay validation).
>   **Chunk 7 (end-to-end legacy-event test) remains** — see `WIP.md` for handoff state.
> - Part E extras (additional readiness checks beyond replay validation) are **out of scope**
>   for the current iteration.

## Path mapping: doc paths → this repo

This document was written from the perspective of `ssi-agent-ext`, which consumes this repo as
the `_core/ssi-agent` submodule. When implementing **in this repo**, map paths as follows:

| Doc path                                            | This repo                                                                                                                                                                                                 |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `_core/ssi-agent/shared-kernel/...`                 | `shared-kernel/`                                                                                                                                                                                          |
| `_core/ssi-agent/infrastructure/stores/mongodb/...` | `infrastructure/stores/mongodb/`                                                                                                                                                                          |
| `_core/ssi-agent/agent_application/...`             | `agent_application/`                                                                                                                                                                                      |
| `src/container.rs` / `CompositionRoot`              | `agent_application/src/lib.rs` (`run()` / `state()` / `router()` / `serve()`)                                                                                                                             |
| `bounded-contexts/*` (bc-trust / bc-verification)   | **ext repo only — out of scope here.** This repo's aggregates are the 19 event enums in `agent_issuance`, `agent_holder`, `agent_verification`, `agent_identity`, `agent_library`, `agent_authorization`. |

Additionally, this repo has **two coexisting store abstractions** (mid-migration), and **both**
get upcaster wiring (decision taken):

1. **Production path** — `agent_store` (used by `agent_application`):
   `CqrsComponentBuilder::commands_and_queries` (`agent_store/src/lib.rs:171`), backends
   `postgres.rs` (via `postgres_es::postgres_cqrs`, which hardcodes no-upcaster construction),
   `mongodb.rs` (`PersistedEventStore::new_event_store(repo)` at `:17`), `in_memory.rs`
   (`MemStore`, cannot apply upcasters — stores envelopes unserialized).
2. **New path** — `shared-kernel/src/command_handler.rs` (`CommandHandlerFactory::create_handler`,
   `:87-100`) implemented by `infrastructure/stores/mongodb/src/lib.rs` (`:55`) and
   `shared-kernel/src/test_utils/in_memory.rs`. Not yet consumed by `agent_application`.

## Context

`ssi-agent-ext` is event-sourced on top of `cqrs-es` (v0.5 for the active bounded
contexts `bc-trust` / `bc-verification`, via the `shared-kernel` re-export). Events are
serde-tagged enums persisted as JSON in MongoDB. Today:

- Every event hardcodes `DomainEvent::event_version() -> "1"` — versioning exists in the
  API but carries no real information.
- **No upcasting is wired.** The Mongo store builds `PersistedEventStore::new_event_store(repo)`
  with no upcaster chain ([\_core/ssi-agent/infrastructure/stores/mongodb/src/lib.rs:55](../_core/ssi-agent/infrastructure/stores/mongodb/src/lib.rs#L55)).
- Replay is **per-aggregate, lazy, on-demand** at command time. A breaking change to an
  event's shape surfaces only as a `PersistenceError::DeserializationError` when that
  specific aggregate is next commanded (or a view processes the event) — not at startup.
- `/healthz` returns `200` unconditionally ([\_core/ssi-agent/agent_application/src/probes/liveness.rs](../_core/ssi-agent/agent_application/src/probes/liveness.rs)),
  so the app advertises "ready" even when persisted events are unreadable.
- Local dev runs the in-memory store (`dev/run.sh` → `--features in-memory-store`), so it
  always starts from an empty log and never exercises pre-existing/legacy events.

The upstream documented policy is literally "wipe the event store on format change"
(`_core/ssi-agent/docs/problem-details/persistence.md`). That is unacceptable for
production deployments that already hold events.

This plan makes the **read side** resilient to breaking event changes via upcasting, and
adds a real readiness gate that refuses to serve if any persisted event cannot be replayed.
API-surface changes are explicitly out of scope. Per decisions taken: changes may land in
the `_core/ssi-agent` submodule (to be upstreamed); the readiness gate does **full replay
validation**; compatibility is exercised via round-trip/upcaster tests rather than
persistent local infra.

## Design overview

Three layers, from cheapest to strongest:

1. **Versioning discipline** — make `event_version` meaningful so upcasters can target it.
2. **Upcasting** — transform old-format events to the current shape on read, using the
   `EventUpcaster` machinery that already ships in `cqrs-es` 0.5 (verified present:
   `EventUpcaster`, `PersistedEventStore::with_upcasters`).
3. **Readiness gate** — at startup, stream and deserialize _every_ persisted event (after
   upcasting) via `stream_all_events::<A>()`; block "ready" on any failure.

Together: upcasting fixes known breaks; the gate detects unknown breaks before traffic.

---

## Part A — Give `event_version` meaning

Currently `event_version()` returns `"1"` for every variant in every aggregate
(`bounded-contexts/*/src/domain/*/event/mod.rs`). Adopt a convention:

- `event_version` is a monotonically increasing integer-like schema version string
  (e.g. `"1"`, `"2"`, `"3"`) for each event payload.
- Establish the rule (document in `docs/`): **never** rename/repurpose an existing variant or
  field in place. To change a payload, bump that event's `event_version` and ship an upcaster
  that maps the old version forward. Additive, optional fields need no bump.

No behavioural change here beyond the version strings; it is the anchor the upcasters key off.

## Part B — Wire upcasting through the store

`cqrs-es` already supports this; the only gap is plumbing per-aggregate upcasters into
`PersistedEventStore::with_upcasters(...)`. The store is built in the generic factory, so
thread upcasters through it.

1. **Extend the factory trait** — `CommandHandlerFactory::create_handler`
   (`_core/ssi-agent/shared-kernel/src/command_handler.rs`) gains a
   `upcasters: Vec<Box<dyn EventUpcaster>>` parameter (re-export `EventUpcaster` via
   `shared_kernel::cqrs_es::persist`).
2. **Apply them in the Mongo store** — at
   [\_core/ssi-agent/infrastructure/stores/mongodb/src/lib.rs:55](../_core/ssi-agent/infrastructure/stores/mongodb/src/lib.rs#L55):
   `PersistedEventStore::new_event_store(repo).with_upcasters(upcasters)`. Do the same for the
   in-memory store's handler construction (`_core/ssi-agent/shared-kernel/src/test_utils/in_memory.rs`)
   so tests exercise upcasters too.
3. **Source upcasters per aggregate** — give each aggregate an upcaster set. Cleanest: a small
   trait method (e.g. `fn upcasters() -> Vec<Box<dyn EventUpcaster>>`) or a plain module
   function per aggregate, collected in each context's builder
   (`bounded-contexts/*/src/application/builder.rs`) and passed into `create_handler`. Empty
   `vec![]` today — non-breaking until the first real migration.
4. **Author upcasters as events evolve** — one `EventUpcaster` implementation per breaking
   change, living next to the event definition and transforming from the old numeric version to
   the next one
   (e.g. `.../event/upcast/*.rs` mirroring the existing `.../event/apply/*` layout).

Representative files: `bc-trust` member/ecosystem/membership and `bc-verification`
verification-flow `event/mod.rs` + builders.

## Part C — Startup readiness gate (full replay validation)

Add a real readiness concept, separate from the existing liveness probe.

1. **Validation pass** — after all `CqrsFramework`s are built but before the HTTP server
   accepts traffic (in `CompositionRoot::start_inner`, `src/container.rs`), run a routine that,
   for each aggregate type, calls `store.stream_all_events::<A>()` and drains the stream. The
   `PersistedEventStore` deserializes each event (applying upcasters first); any
   `PersistenceError::DeserializationError` fails the pass. This replays the entire read side
   without needing aggregate IDs. Expose a helper on the store factory (e.g.
   `validate_events::<A>()`) so the generic call site stays clean; iterate it over every
   aggregate type registered by the two context builders.
2. **Readiness state + probe** — hold an `AtomicBool`/`watch` "ready" flag set to `true` only
   after the validation pass succeeds. Add `GET /readyz` alongside `healthz`
   (`_core/ssi-agent/agent_application/src/probes/`, wired in `agent_application/src/lib.rs`
   next to the existing `/healthz` route) returning `200` when ready, `503` otherwise.
   Keep `/healthz` as pure liveness.
3. **Startup behaviour** — on validation failure, log the offending `aggregate_type` /
   `event_type` / `event_version` and either (a) refuse to become ready (stay `503`, safe
   default) or (b) hard-exit. Recommend **stay-503 + loud error** so orchestrators hold the
   old revision instead of routing to a broken one. Gate the whole pass behind a config flag
   defaulting to on, so a very large store can opt out if boot time becomes a problem.

This directly answers "should all events be read before the app claims ready?" — yes, behind
`/readyz`, so a bad deploy is caught at rollout instead of at first request.

## Part D — Tests (compatibility safety net, no local infra)

Since local dev stays on the clean in-memory store, compatibility is guaranteed by tests, not
by prod-like local data:

1. **Round-trip tests** per aggregate: serialize each event variant to JSON and deserialize
   back, asserting equality. This locks the wire format and makes any accidental breaking
   change fail CI. None exist today — add under each `event/` module.
2. **Golden/legacy fixtures**: check in JSON snapshots of the _current_ event formats. When a
   format is later changed + upcasted, the old fixture must still deserialize (through the
   upcaster) to the new type — this is the regression test for each upcaster.
3. **Upcaster unit tests**: feed a v-old `SerializedEvent`, assert the upcaster yields the
   v-new payload (mirror the `EventUpcaster` doctest pattern in
   `cqrs-es-0.5.0/src/persist/upcaster.rs`).
4. **Extend the smoke test** (`tests/smoke.rs`): seed the in-memory store with a legacy-format
   event, boot `CompositionRoot`, and assert `/readyz` reaches `200` (proves the gate +
   upcaster path run end-to-end).

## Files to modify

- `_core/ssi-agent/shared-kernel/src/command_handler.rs` — add `upcasters` param + re-export `EventUpcaster`.
- `_core/ssi-agent/infrastructure/stores/mongodb/src/lib.rs` — `.with_upcasters(...)`; add `validate_events` helper.
- `_core/ssi-agent/shared-kernel/src/test_utils/in_memory.rs` — mirror upcaster + validation for in-memory.
- `_core/ssi-agent/agent_application/src/probes/` + `.../src/lib.rs` — add `/readyz` + readiness flag.
- `src/container.rs` — run the validation pass, own the readiness flag, sequence it before serving.
- `bounded-contexts/*/src/application/builder.rs` — collect per-aggregate upcasters, pass to `create_handler`; register aggregate types for the validation loop.
- `bounded-contexts/*/src/domain/*/event/mod.rs` (+ new `event/upcast/*.rs`) — semantic `event_version`, upcasters, round-trip tests.
- `docs/` — document the versioning/upcasting rule and the "wipe" policy replacement.

## Part E — What else `/readyz` should check

The event-replay validation (Part C) is the headline check, but readiness should reflect
_every hard dependency the app needs to serve a request_. Group into checks that gate
readiness vs. dependencies that deliberately must **not** gate it.

### Should gate `/readyz` (fail → `503`)

1. **Event-store connectivity** — a MongoDB ping. The replay pass implicitly needs this, but
   check it explicitly so an unreachable store yields a clear, fast `503` rather than a slow
   stream error. Store built in `CompositionRoot::start` (`src/container.rs`) via
   `MongoDBStore::new(connection_string)`.
2. **Read-model / view store reachable** — views live in the same Mongo but separate
   collections. Confirm with a `ViewRepository::load(<known list-view id>)` (e.g.
   `ECOSYSTEM_LIST_VIEW_ID`, `VERIFICATION_FLOW_LIST_VIEW_ID`) so a broken projection store is
   caught, not just the event log.
3. **Secret manager / `Subject` usable** — `Subject::new().await` (`src/container.rs:83`) loads
   the Stronghold snapshot and signing key. Readiness should confirm the subject can actually
   produce a signature / resolve its DID; a wrong Stronghold password or missing snapshot means
   the app can boot but cannot sign (issuance, OIDF entity statements) — it must not be "ready".
4. **Startup seeding completed** — the core `initialize()` functions
   (`agent_issuance::state::initialize`, `agent_identity`, `agent_authorization`, plus
   `initialize_domain_linkage` / `initialize_linked_verifiable_presentations`) seed the
   `server_config` and identity views. They currently `.unwrap()` and would panic on failure;
   fold them into readiness instead (e.g. assert the `server_config` view is present) so a
   seeding failure degrades to `503` rather than a crash loop.
5. **Bounded-context service tasks alive** — the Trust and Verification services run as `mpsc`
   consumer loops started in the `tokio::join!` (`src/container.rs:196`). A cheap query-envelope
   round-trip (e.g. list-view query) confirms both the channel consumer is running _and_ the
   view store answers. If a service task has died, its channel round-trip times out → `503`.
6. **HTTP listener bound** — flip the ready flag only _after_ `axum::serve` has bound the
   listener and routes are mounted (`presentation/api-http/src/lib.rs:71`).

### Should **not** gate `/readyz` (avoid cascading unreadiness)

These are outbound calls to _other_ services; letting them fail readiness would take this pod
down whenever a downstream is briefly unavailable, which cascades and is rarely what you want:

- **OpenID Federation metadata resolver** (`OidfMetadataResolver`) and remote entity-statement
  fetches — outbound HTTP to peers.
- **Member invitation sender** (`HttpMemberInvitationSender`) and **trust anchor notifier**
  (`HttpTrustAnchorNotifier`) — outbound webhooks.
- **`public_url` self-reachability** — do not probe your own external URL from readiness.

Surface these under a separate diagnostic endpoint (e.g. `/healthz/dependencies`) or metrics
if you want visibility, but keep them out of the readiness gate. Config validation (`config()`)
belongs at startup — invalid config should fail the process, not `/readyz`.

### Implementation note

Model readiness as a set of named checks behind the single `AtomicBool`/`watch` flag from
Part C: run the one-shot checks (1, 3, 4, replay validation) once at boot before flipping
ready; run the cheap liveness-of-dependency checks (2, 5) either once at boot or on each
`/readyz` hit (they are fast). Return a small JSON body listing each check's pass/fail so a
`503` is diagnosable.

## Part F — Shutdown behaviour (the delay is _missing_ signal handling, not graceful shutdown)

> **✅ DONE — implemented and validated in this repo.**
> Commit `b32ea054 feat: handle termination signal`. A `shutdown_signal()` helper (SIGTERM on
> unix + Ctrl-C/SIGINT) is raced via `tokio::select!` against the server tasks in `serve()`
> (`agent_application/src/lib.rs:314-365`); on signal the process logs
> `"Shutdown signal received, exiting immediately."` and returns cleanly (exit 0 via runtime
> drop — no `process::exit`, no Dockerfile change). Measured locally: **~136 ms** from
> `kill -TERM` to process exit (previously: ignored SIGTERM, killed only after the k8s grace
> period). Remaining container-level checks (`docker kill -s TERM`, `kubectl delete pod`) are
> deployment-side verification only.

**Root cause of the multi-second shutdown.** The container runs the binary as **PID 1** —
`Dockerfile` uses exec-form `ENTRYPOINT ["/usr/local/bin/ssi-agent-ext"]` with no init
(`tini`/`dumb-init`). The code installs **no signal handler anywhere** (`axum::serve(...).await`
has no `.with_graceful_shutdown`, and there is no `tokio::signal` usage in the tree). For PID 1
the Linux kernel does **not** apply default signal dispositions, so a process only reacts to
signals it explicitly handles. Kubernetes sends `SIGTERM`, the app **ignores** it, and k8s then
waits out `terminationGracePeriodSeconds` before sending `SIGKILL`. The "few seconds" you see is
that grace period elapsing — there is currently **no graceful shutdown to skip**.

**So "can we skip graceful shutdown entirely?" → effectively yes, and it's the recommended
target — but you get there by _adding_ a minimal fast-exit handler, not by removing anything.**

Two ways to make the process exit promptly on `SIGTERM`:

1. **In-app fast-exit handler (recommended).** Add one more branch to the `tokio::join!` in
   `CompositionRoot::start_inner` (`src/container.rs:196`) that awaits `SIGTERM`/`SIGINT` and
   exits immediately, skipping any drain:

   ```rust
   async {
       use tokio::signal::unix::{signal, SignalKind};
       let mut term = signal(SignalKind::terminate()).unwrap();
       tokio::select! {
           _ = term.recv() => {},
           _ = tokio::signal::ctrl_c() => {},
       }
       tracing::info!("Signal received, shutting down immediately.");
       std::process::exit(0);
   }
   ```

   This is portable (no image change) and gives near-instant termination. Because `select!`
   inside `join!` drops the other futures and `process::exit` bypasses runtime teardown, nothing
   waits.

2. **Add an init to the image.** Set `docker run --init`, or an explicit `tini`/`dumb-init`
   entrypoint, so PID 1 becomes the init and default `SIGTERM` disposition applies to the child.
   Fixes the PID-1 signalling issue but still relies on default terminate; the in-app handler is
   more explicit and also covers local runs.

Optionally also set a low `terminationGracePeriodSeconds` in the k8s manifest as a backstop —
but on its own it does not fix ignored `SIGTERM`, it only shortens the wait before `SIGKILL`.

**Is immediate exit safe here?** For this event-sourced app, yes:

- **Writes are atomic appends.** Each command persists its events as an atomic MongoDB insert
  (with an optimistic-concurrency sequence). A hard kill either happened before that append
  (command simply lost — client retries) or after (fully committed). There is no partially
  written aggregate state.
- **No in-memory state to flush.** Aggregates are rehydrated per command; read-model views are
  persisted through the store per event, not buffered in memory. Nothing is lost by not draining.
- **In-flight HTTP requests** get a dropped connection; clients retry. For OID4VP handshakes the
  wallet restarts/retries the flow. This is the only user-visible effect, and it is acceptable.
- **Verify once:** confirm the Stronghold/secret manager does no unflushed background writes
  (it is read-mostly — signing loads key material from the snapshot at boot). If a future change
  makes it write at runtime, revisit whether an immediate exit can lose a snapshot update.

If you later want _drain-then-exit_ instead of hard exit (e.g. to let in-flight requests finish),
switch `axum::serve(...)` to `.with_graceful_shutdown(signal_future)` in
`presentation/api-http/src/lib.rs` — but given the requirement is faster shutdown, the fast-exit
handler is the right default.

## Verification

1. `cargo build --workspace` and `cargo test --workspace` (round-trip, upcaster, smoke tests pass).
2. **Baseline still ready**: `cargo run --release --features in-memory-store` (`dev/run.sh`),
   then `curl -s -o /dev/null -w '%{http_code}' localhost:<port>/readyz` → `200`.
3. **Breaking change caught**: in a test, persist an event whose payload no longer matches the
   current struct _without_ an upcaster; assert the validation pass errors and `/readyz` stays
   `503`. Add the matching upcaster; assert it flips to `200`.
4. **Round-trip guard works**: temporarily rename an event field, confirm the round-trip test
   fails (proving CI would block an un-upcasted breaking change), then revert.
5. Confirm `/healthz` behaviour is unchanged (still unconditional `200`).
6. **Readiness reflects a broken dependency**: point the app at an unreachable Mongo, confirm
   `/readyz` returns `503` with a body naming the failing check while `/healthz` stays `200`.
7. **Fast shutdown**: run the container, send `SIGTERM` (`kubectl delete pod` or
   `docker kill -s TERM`), and confirm the process exits in well under a second instead of
   sitting until the grace period / `SIGKILL`. ✅ _Done — validated locally (~136 ms exit), see
   Part F status._

---

## Implementation plan (this repo, chunked for sub-agents)

Parts A–D broken into small, self-contained work packages. Execute in waves; chunks within a
wave are independent and can run as parallel sub-agents. Every chunk ends with a scoped
`cargo build` + `cargo test` green, and one conventional commit on `feat/upcast-events`.

### Architecture facts every chunk needs

- Crates: `cqrs-es 0.5.0`, `postgres-es 0.5.0`, `mongo-es 0.4.1` (crates.io, unpatched).
  Verified available in `cqrs-es-0.5.0` sources:
  - `EventUpcaster` trait — `src/persist/upcaster.rs:10`
  - `PersistedEventStore::with_upcasters(Vec<Box<dyn EventUpcaster>>)` — `persist/event_store.rs:170`
  - `PersistedEventRepository::stream_all_events::<A>()` — `persist/event_repository.rs:43`
  - `QueryReplay` with `.with_upcasters()` / `.replay_all()` — `persist/replay.rs`
- `shared-kernel` re-exports `cqrs_es` (`shared-kernel/src/lib.rs:26`).
- The 19 event enums (all with hand-written, hardcoded `event_version() -> "1"`; no macro):
  - `agent_issuance`: `offer/event.rs:67`, `credential/event.rs:50`, `nonce/event.rs:16`,
    `server_config/event.rs:55`, `status_list/event.rs:33`, `public_offer/event.rs:30`
  - `agent_holder`: `offer/event.rs:42`, `credential/event.rs:23`, `presentation/event.rs:19`
  - `agent_verification`: `authorization_request/event.rs:34`
  - `agent_identity`: `connection/event.rs:43`, `document/event.rs:47`, `service/event.rs:38`,
    `profile/event.rs:44`
  - `agent_library`: `template/event.rs:95`
  - `agent_authorization`: `access_token/event.rs:24`, `client/event.rs:31`,
    `authorization_code/event.rs:29`, `oauth2_authorization_request/event.rs:53`
- Probes: only `/healthz` (unconditional 200, `agent_application/src/probes/liveness.rs:5`);
  router assembled in `agent_application/src/lib.rs:259-288`.
- No event serialization tests exist anywhere (greenfield for Part D).

### Wave 1 (parallel)

#### Chunk 1 — Integer event versions + empty upcaster scaffolds + docs (Part A)

_Mechanical; one agent; no dependencies._

- In all 19 event files: keep `event_version()` at `"1"` initially (or set to `"1"` where needed).
  Then increment with each breaking payload change (`"2"`, `"3"`, ...). (Safe: the stored
  version string is only consulted by upcasters; none exist yet.)
- Next to each event enum, add a scaffold fn:
  `pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> { vec![] }`
  with a doc comment pointing at the convention doc.
- Add `docs/event-versioning.md`: `event_version` is an integer schema version per event payload;
  never rename/repurpose a variant or field in place; breaking payload change = bump version +
  ship an `EventUpcaster` next to the event; additive optional fields need no
  bump. Replaces the old "wipe the event store" policy.
- **Acceptance:** workspace builds; grep shows no remaining `"1".to_string()` in `event_version`.

#### Chunk 2 — shared-kernel: factory signature + in-memory persisted repo test util (Part B.1)

_One agent; no dependencies._

- `shared-kernel/src/command_handler.rs`: add `upcasters: Vec<Box<dyn EventUpcaster>>`
  parameter to `CommandHandlerFactory::create_handler` (`:87-100`). Re-export `EventUpcaster`
  (`cqrs_es` is already re-exported at `lib.rs:26`).
- `shared-kernel/src/test_utils/in_memory.rs`: `InMemoryStore` currently builds
  `CqrsFramework` over `MemStore`, which cannot apply upcasters. Implement a minimal in-memory
  `cqrs_es::persist::PersistedEventRepository` (HashMap of `SerializedEvent`s behind a
  `RwLock`) and build the test handler as
  `PersistedEventStore::new_event_store(repo).with_upcasters(upcasters)` so tests genuinely
  exercise the upcaster path. Keep `ViewRepositoryFactory` behaviour unchanged.
- Update all in-repo callers of `create_handler` to pass `vec![]` (compile-fix pass).
- **Acceptance:** `cargo test -p shared-kernel` green; a new unit test proves an upcaster runs
  on read through the in-memory persisted repo (mirror the `EventUpcaster`
  doctest in `cqrs-es-0.5.0/src/persist/upcaster.rs`).

### Wave 2 (parallel, after Wave 1)

#### Chunk 3 — infrastructure/stores/mongodb: upcasters + validate helper (Part B.2, new path)

_One agent; depends on Chunk 2 (new factory signature)._

- `infrastructure/stores/mongodb/src/lib.rs:55`:
  `PersistedEventStore::new_event_store(repo).with_upcasters(upcasters)` from the new
  `create_handler` parameter.
- Add a `validate_events::<A>()` helper on the store: drain `stream_all_events::<A>()`
  (upcasters applied by the store/replay machinery — prefer `QueryReplay`-style streaming),
  mapping any `PersistenceError` into a descriptive error naming
  `aggregate_type`/`event_type`/`event_version`.
- **Acceptance:** crate builds + unit tests; helper usable without a live Mongo in tests
  (integration against real Mongo is deferred to the ext repo / CI).

#### Chunk 4 — agent_store: thread upcasters through all three backends (Part B.2, production path)

_One agent; depends on Chunk 1 (per-aggregate `upcasters()` fns)._

- Extend `CqrsComponentBuilder::commands_and_queries` (`agent_store/src/lib.rs:171`) — or
  `AggregateHandler::new` — to accept `Vec<Box<dyn EventUpcaster>>`.
- `agent_store/src/mongodb.rs:17`: `.with_upcasters(upcasters)` on the `PersistedEventStore`.
- `agent_store/src/postgres.rs:16`: replace `postgres_es::postgres_cqrs(...)` with manual
  assembly: `PostgresEventRepository::new(pool)` →
  `PersistedEventStore::new_event_store(repo).with_upcasters(upcasters)` →
  `CqrsFramework::new(store, queries, services)` (mirror `postgres-es-0.5.0/src/cqrs.rs:30-39`,
  plus upcasters).
- `agent_store/src/in_memory.rs`: accept the parameter; `MemStore` cannot apply upcasters —
  document this limitation in a comment (upcaster behaviour is covered by shared-kernel's
  persisted in-memory repo from Chunk 2).
- Wire the six `*_state` fns (`identity_state:181`, `library_state:230`,
  `authorization_state:261`, `issuance_state:317`, `verification_state:390`,
  `holder_state:420`) to pass each aggregate's `upcasters()` from Chunk 1.
- **Acceptance:** workspace builds; `cargo test -p agent_store -p agent_application` green.

#### Chunk 5 — Round-trip + golden fixture tests (Part D.1/D.2)

_Mechanical; splittable across 2 parallel agents (issuance+library+holder /
identity+verification+authorization); depends only on Chunk 1._

- Per event enum: a round-trip test module — construct each variant (minimal fixture values),
  `serde_json::to_value` → `from_value`, assert equality. Locks the wire format in CI.
- Golden fixtures: JSON snapshots of each variant's _current_ serialized form (e.g.
  `tests/fixtures/events/<aggregate>/<variant>.json` or inline `serde_json::json!` literals);
  test asserts today's deserializer accepts them. When a format later changes + gets an
  upcaster, the old fixture becomes that upcaster's regression input.
- Follow existing test style (aggregate `TestFramework` tests in each `aggregate.rs`;
  integration style in `agent_issuance/tests/credential_configuration_projection.rs`).
- **Acceptance:** `cargo test` green across the six agent crates; deliberately renaming one
  field locally makes the round-trip test fail (spot-check, then revert).

### Wave 3 (after Wave 2)

#### Chunk 6 — Readiness gate: replay validation + /readyz (Part C)

_One agent; depends on Chunk 4._

- Production-path validation: expose per-backend `validate_events` (Postgres/Mongo: drain
  `stream_all_events::<A>()` with upcasters applied; InMemory: `Ok(())`). Cleanest as a method
  on `CqrsComponentBuilder` or a parallel small trait in `agent_store`.
- `agent_application/src/lib.rs`: after `state()` builds all six states, run the validation
  pass over every aggregate type, behind a config flag (`event_replay_validation` in
  `agent_shared/src/config/mod.rs`, default **on**; follow existing `#[config(default…)]`
  patterns). On failure: log `aggregate_type`/`event_type`/`event_version` loudly and leave
  the app **not-ready** (do not exit).
- Readiness state: `Arc<AtomicBool>` (or `tokio::sync::watch`) set true only after validation
  passes. Add `probes/readiness.rs` with `readyz()` returning `200` when ready else `503`
  with a small JSON body naming the failing check; route `/readyz` next to `/healthz` in
  `router()` (`agent_application/src/lib.rs:285`). `/healthz` stays unconditional.
- Mirror `validate_events` for the shared-kernel path if Chunk 3's helper needs adjusting.
- **Acceptance:** boot with in-memory store → `/readyz` = 200, `/healthz` = 200; unit test for
  the 503-until-validated flip.

### Wave 4 (last)

#### Chunk 7 — End-to-end legacy-event test (Part D.3/D.4)

_One agent; depends on Chunks 2, 4, 6._

- Upcaster unit test: feed a hand-built v-`1` `SerializedEvent` with an _old_ payload
  shape to a demo `EventUpcaster` from version `"1"` to `"2"`; assert the upcasted JSON
  parses into the current enum (mirror the cqrs-es doctest).
- Integration test: using shared-kernel's in-memory persisted repo (Chunk 2), seed a
  legacy-shaped event, register the demo upcaster, run the replay-validation routine, assert
  it passes; remove the upcaster, assert it fails with the descriptive error. If feasible,
  extend to a boot-level test asserting `/readyz` reflects both outcomes.
- **Acceptance:** `cargo test --workspace` green.

### Orchestration notes

- Wave 1: Chunks 1 & 2 in parallel → Wave 2: Chunks 3, 4, 5 in parallel → Wave 3: Chunk 6 →
  Wave 4: Chunk 7.
- Each sub-agent prompt should carry: the chunk text verbatim, the "Architecture facts"
  section, and the path-mapping table. Sub-agents must not touch this document or unrelated
  files, and must run the scoped builds/tests before finishing.
- Suggested commits: `feat(events): integer event versions`,
  `feat(shared-kernel): upcaster-aware command handler factory`,
  `feat(store): wire event upcasters`, `feat(probes): add /readyz with replay validation`,
  `test(events): round-trip and golden fixture coverage`.
