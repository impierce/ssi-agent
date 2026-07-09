# Making `feat/upcast-events` reviewable — analysis & options

Review of `feat/upcast-events` against `origin/beta`, 2026-07-08.

## Where the ~5,070 added lines come from

| Portion | Lines | Share | Needs real review? |
|---|---|---|---|
| 19× `event.rs` boilerplate (comment + empty `upcasters()` + golden tests) | ~2,770 | 55% | No — same template repeated |
| `tmp/` planning notes (`EVENT_BREAKING_CHANGES.md`, `RESULTS.md`, `WIP.md`) | ~640 | 13% | No — working notes |
| Substance: shared-kernel plumbing, `agent_store` wiring + `validation.rs`, `/readyz`, `legacy_event_replay`, `docs/event-versioning.md` | ~1,650 | 32% | **Yes** |

Each of the 19 `event.rs` files gets the same three additions:

1. A comment on `event_version()` ("bump on breaking change…")
2. `pub fn upcasters() -> Vec<Box<dyn EventUpcaster>> { vec![] }` — **all 19 are empty**
3. A hand-written golden-JSON test module, 50–320 lines each

## Core question: defer wire-format tests until an upcaster is needed?

**No — that inversion defeats the purpose.** The golden tests *are* the detection
mechanism: they trip when someone changes an event's serialized shape, which is
the moment you learn an upcaster is needed. Remove them until then and nothing
warns you.

But the idea **does** work for the empty `upcasters()` functions (no detection
role), and the tests themselves can shrink ~90%.

## Options for shrinking the diff

### 1. Drop the 19 empty `upcasters()` functions ⭐ recommended

Defer each `upcasters()` until the first real upcaster exists. Either pass
`Vec::new()` at the `agent_store` call sites, or add a trait with a default in
shared-kernel:

```rust
pub trait UpcastableEvent: DomainEvent {
    fn upcasters() -> Vec<Box<dyn EventUpcaster>> { vec![] }
}
```

Event files then need zero code until they override it. Removes 19 functions +
doc comments, and makes "this aggregate has a migration" greppable.

### 2. `insta` snapshot tests instead of hand-written `json!` goldens ⭐ recommended

Per event file: an `all_variants()` constructor + a ~5-line loop calling
`insta::assert_json_snapshot!`. Golden JSON moves to generated `.snap` files
reviewers can skim/skip. This branch doesn't change serialization, so snapshots
generated now are faithful to what beta persists.

Caveat: `cargo insta accept` makes updating goldens one keystroke — the
"is this drift deliberate?" check moves to reviewing `.snap` diffs in CI.
Team should adopt that convention explicitly.

### 3. `wire_format_tests!` macro in shared-kernel `test_utils`

Middle ground: macro takes the event type + `(variant, golden)` pairs and
generates the harness (`assert_round_trip_and_golden` etc.). Keeps explicit
`json!` goldens in-file, saves ~25 harness lines per file. No new dependency,
smaller win than insta.

### 4. External JSON fixture files + one generic test per crate

Goldens live in `tests/fixtures/*.json`; a single test iterates the directory
and round-trips each through the event enum. Similar effect to insta without
the dependency, but you build the harness yourself.

## Options for PR structure

- **Remove `tmp/`** — 640 lines of planning notes; the useful content already
  graduated into `docs/event-versioning.md`.
- **Stack two PRs** ⭐ recommended:
  - **PR 1 (mechanism, ~1,650 lines):** shared-kernel changes, `agent_store`
    wiring + `validation.rs`, `/readyz`, `docs/event-versioning.md`,
    `legacy_event_replay` test.
  - **PR 2 (mechanical rollout):** per-crate test additions — "review one file
    carefully, the rest are the same template."
- **Or single PR with commit-per-crate hygiene** — e.g.
  `test: wire-format goldens for agent_issuance` — plus a PR description naming
  the 4–5 files to review closely.
- **Unify naming:** 10 files use `mod wire_format_tests`, the identity/
  authorization crates use `mod event_tests`, with slightly different assertion
  styles. One name + one shared helper makes "the rest are identical" true.

## Recommendation

Combine **1 + 2** (trait-default upcasters, insta snapshots), **delete `tmp/`**,
and **split into two stacked PRs**. Reviewable surface drops from ~5,000 lines
to ~1,650 lines of real logic plus a mechanical PR of roughly 400 lines of Rust
and skimmable snapshot files — with the same drift-detection guarantee.
