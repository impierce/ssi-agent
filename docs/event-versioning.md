# Event Versioning

This document describes how UniCore versions the payload schema of persisted domain events, and how breaking changes to an event's shape are migrated forward.

This replaces the previous approach of wiping the event store whenever an event's format changed (see [Deserialization Error](./problem-details/persistence.md#deserialization-error)). Event stores now persist across schema changes, provided the convention below is followed.

## The convention

Every event enum implements `cqrs_es::DomainEvent`, which requires an `event_version(&self) -> String`. In UniCore, this version is a **monotonically increasing integer, encoded as a string**: `"1"`, `"2"`, `"3"`, and so on. It is **not** a semantic version — there is no major/minor/patch structure, and versions are compared by parsing the integer, not by semver rules.

Every event enum starts at version `"1"`. That is the version already hard-coded in each `event_version()` implementation today.

### When you must bump the version

Bump `event_version` and ship an upcaster when a change to an event variant's payload is **breaking** for events already persisted in production — for example:

- renaming or removing a field
- changing a field's type or meaning
- renaming an enum variant
- removing a variant that still has persisted instances

**Never rename or repurpose an existing variant or field in place.** If a field or variant needs a different name or meaning, that is a breaking change: add a new one and migrate via an upcaster instead of mutating the old one, otherwise old persisted JSON will silently (de)serialize into the wrong shape.

### When you do not need to bump the version

Purely additive, backward-compatible changes do **not** require a version bump:

- adding a new `Option<T>` field (deserializes to `None` for old events)
- adding a new field annotated with `#[serde(default)]`
- adding a new enum variant that no old event will ever deserialize into

### How to bump a version

1. Change the event payload as needed.
2. Bump the `event_version()` return value for that event enum, e.g. `"1"` → `"2"`.
3. Write a hand-implemented `cqrs_es::persist::EventUpcaster` that transforms the *old* serialized payload into the *new* one, and bumps the serialized event's `event_version` to match. Register it in that aggregate's `upcasters()` function, exported next to the event enum in its `event.rs` file.

UniCore does **not** use `cqrs_es`'s built-in `SemanticVersionEventUpcaster` — that type parses versions as semver (`major.minor.patch`), which does not match our plain-integer versioning. Upcasters are instead implemented directly against the `EventUpcaster` trait:

```rust
pub trait EventUpcaster: Send + Sync {
    /// Examines an event type and version to understand if the event should be upcasted.
    fn can_upcast(&self, event_type: &str, event_version: &str) -> bool;

    /// Modifies the serialized event to conform to the new structure.
    fn upcast(&self, event: SerializedEvent) -> SerializedEvent;
}
```

`can_upcast` is matched against the *stored* event's `event_type` and `event_version`; `upcast` receives the raw `SerializedEvent` (JSON payload plus metadata) and must return a `SerializedEvent` whose payload matches the new schema and whose `event_version` reflects the new version. Upcasters run in the event store when events are read back, before deserialization into the typed `DomainEvent`, so every consumer (aggregates, queries, replays) sees only the current shape.

### Where upcasters live

Each aggregate's `event.rs` file exports a `pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>>` next to its event enum. This starts out empty (`vec![]`) and gains one entry per version bump, in order.

## Example

Given `event_version` bumped from `"1"` to `"2"` because `AccountOpened` gained a required `currency` field:

```rust
struct AddDefaultCurrencyUpcaster;

impl EventUpcaster for AddDefaultCurrencyUpcaster {
    fn can_upcast(&self, event_type: &str, event_version: &str) -> bool {
        event_type == "AccountOpened" && event_version == "1"
    }

    fn upcast(&self, event: SerializedEvent) -> SerializedEvent {
        let mut payload = event.payload;
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("currency".to_string(), serde_json::json!("USD"));
        }
        SerializedEvent {
            event_version: "2".to_string(),
            payload,
            ..event
        }
    }
}

pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![Box::new(AddDefaultCurrencyUpcaster)]
}
```
