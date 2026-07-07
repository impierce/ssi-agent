# WIP — Event upcasting + readiness gate (`feat/upcast-events`)

State as of 2026-07-07 (second session): **all 7 chunks of the plan are implemented and
verified.** Plan of record: `EVENT_BREAKING_CHANGES.md` (§ "Implementation plan"). Part F
(SIGTERM fast shutdown) landed earlier (`b32ea054`).

## Done (all chunks)

| Chunk | What landed | Where |
|---|---|---|
| 1 | Integer event versioning convention, empty `pub fn upcasters()` scaffold next to all 19 event enums, convention doc | 19× `*/event.rs`, `docs/event-versioning.md` — committed `264f763c` |
| 2 | `CommandHandlerFactory::create_handler(..., upcasters)`; in-memory `PersistedEventRepository` in test utils | `shared-kernel/src/command_handler.rs`, `shared-kernel/src/test_utils/in_memory.rs` — committed `264f763c` |
| 3 | `.with_upcasters(...)` wiring + `validate_events::<A>()` for the shared-kernel Mongo path | `infrastructure/stores/mongodb/src/lib.rs` — committed (Wave 2) |
| 4 | Upcasters threaded through `CqrsComponentBuilder::commands_and_queries(...)` for all 3 backends, all 19 call sites | `agent_store/src/{lib,postgres,mongodb,in_memory}.rs` — committed (Wave 2) |
| 5 | Round-trip + golden-JSON wire-format tests for all 78 variants of all 19 event enums | test mods in each `*/event.rs` — committed (Wave 2) |
| 6 | Startup replay validation + `/readyz` (`agent_store::validation`, `Readiness` handle, `event_replay_validation` config flag, default on; on failure app stays up, `/readyz` = 503 with reason) | `agent_store/src/validation.rs`, `agent_application/src/probes/readiness.rs`, `agent_application/src/lib.rs`, `agent_shared/src/config/mod.rs` — **staged** |
| 7 | End-to-end legacy-event tests (this session) | see below — **staged** |

### Chunk 7 specifics (staged)

- `agent_issuance/src/nonce/event.rs` § `upcaster_tests`: hand-built version-`"1"`
  `SerializedEvent` with an old payload shape (no `is_redeemed`) + demo
  `SemanticVersionEventUpcaster` `"1"` → `"2"`; asserts the legacy payload is rejected as-is,
  parses into the current `NonceEvent` after upcasting, and that `can_upcast` gates correctly.
  Note: the helper stamps `"2.0.0"` (Display of the parsed version) — harmless, doc updated.
- `agent_store/tests/legacy_event_replay.rs`: seeds a legacy + a current event into
  shared-kernel's `InMemoryEventRepository`, runs `agent_store::validation::validate_event_stream`
  through the real `stream_all_events` path; asserts failure with the descriptive
  `EventValidationError` without the upcaster and `Ok(2)` with it.
  (`agent_store` gained dev-dep `shared-kernel` with `test-utils`.)
- `agent_application/src/lib.rs` § `tests` (boot-level): `run_startup_validation` +
  `/readyz` via `tower::oneshot` — Ok(42) → 200 `ready`; a real `EventValidationError` → 503
  with the exact descriptive reason string. (`agent_application` gained dev-deps
  `agent_shared` with `test_utils` and `cqrs-es`.)
- `cargo fmt --all` was run: it also reformatted some Wave 1/2 files that were committed with
  violations (CI enforces `cargo fmt --all -- --check`); those fixes are staged alongside.

## Verified

- `cargo test --workspace`: **446 passed, 0 failed** (439 before + 7 new). `cargo clippy
  --workspace --all-targets` clean, `cargo fmt --check` clean.
- **Live `/readyz` boot check completed**: booted the debug binary with
  `UNICORE__APPLICATION_URL=http://localhost:3044 UNICORE__METRICS__ENABLED=false
  UNICORE__EVENT_STORE__TYPE=in_memory ./target/debug/agent_application` (repo root as CWD).
  Log shows `Event replay validation succeeded: 0 event(s)`; `curl /readyz` → `200
  {"status":"ready"}`, `/healthz` → 200. SIGTERM shut it down instantly.
  - Root cause of last session's failed check: the bind port comes from
    `config().application_url` → env key **`UNICORE__APPLICATION_URL`** (the repo `.env` pins it
    to `http://192.168.1.124:3033`); `UNICORE__URL` is not a config key. Real env vars override
    `.env` (dotenvy doesn't overwrite existing vars).

## Remaining

1. **Commit the staged work** (Daniel does this himself). Suggested:
   `feat(probes): add /readyz with startup event-replay validation` +
   `test(events): end-to-end legacy-event upcasting coverage` (or one commit).
2. Optional follow-ups deliberately out of scope: Part E extra readiness checks; wiring
   `agent_application` onto the shared-kernel store path (two store abstractions coexist).
3. Untracked scratch docs at repo root: `EVENT_BREAKING_CHANGES.md`, `UTOIPA_EXTERNAL_STRUCTS.md`,
   this `WIP.md` — intentionally uncommitted; delete when the branch is wrapped up.
