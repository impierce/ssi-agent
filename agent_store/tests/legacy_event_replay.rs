//! End-to-end regression test for the legacy-event migration path (`docs/event-versioning.md`):
//! a legacy-shaped event seeded into a persisted-event repository passes the startup replay
//! validation only when the matching upcaster is registered, and fails with the descriptive
//! [`EventValidationError`] otherwise.
//!
//! Uses `shared-kernel`'s [`InMemoryEventRepository`], which genuinely (de)serializes events
//! through [`SerializedEvent`], so the validation routine exercises the exact same
//! stream → upcast → deserialize pipeline it runs against Postgres/MongoDB on startup.

use agent_issuance::nonce::aggregate::Nonce;
use agent_issuance::nonce::event::NonceEvent;
use agent_store::validation::validate_event_stream;
use cqrs_es::persist::{
    EventUpcaster, PersistedEventRepository, PersistenceError, SemanticVersionEventUpcaster, SerializedEvent,
};
use cqrs_es::Aggregate;
use serde_json::json;
use shared_kernel::test_utils::in_memory::InMemoryEventRepository;

/// A `NonceGenerated` event as it would sit in the event store if the version-`"1"` payload had
/// lacked the `is_redeemed` field (a hypothetical breaking change, bumped to `"2"` below).
fn legacy_nonce_generated(sequence: usize) -> SerializedEvent {
    SerializedEvent::new(
        "nonce-1".to_string(),
        sequence,
        Nonce::TYPE.to_string(),
        "NonceGenerated".to_string(),
        "1".to_string(),
        json!({ "NonceGenerated": { "c_nonce": "legacy-c-nonce" } }),
        json!({}),
    )
}

/// A `NonceGenerated` event in the current shape, stored under the post-bump version `"2"`.
fn current_nonce_generated(sequence: usize) -> SerializedEvent {
    SerializedEvent::new(
        "nonce-1".to_string(),
        sequence,
        Nonce::TYPE.to_string(),
        "NonceGenerated".to_string(),
        "2".to_string(),
        serde_json::to_value(NonceEvent::NonceGenerated {
            c_nonce: "current-c-nonce".to_string(),
            is_redeemed: false,
        })
        .unwrap(),
        json!({}),
    )
}

/// Demo upcaster for the hypothetical `"1"` → `"2"` bump: defaults the missing `is_redeemed`
/// field to `false`.
fn demo_upcaster() -> Box<dyn EventUpcaster> {
    Box::new(SemanticVersionEventUpcaster::new(
        "NonceGenerated",
        "2",
        Box::new(|mut payload| {
            if let Some(inner) = payload
                .get_mut("NonceGenerated")
                .and_then(|inner| inner.as_object_mut())
            {
                inner.insert("is_redeemed".to_string(), json!(false));
            }
            payload
        }),
    ))
}

/// Seeds one legacy-shaped and one current-shaped event (in that order, under the same aggregate
/// instance so the replay order is deterministic).
async fn seeded_repository() -> InMemoryEventRepository {
    let repository = InMemoryEventRepository::default();
    repository
        .persist::<Nonce>(&[legacy_nonce_generated(1), current_nonce_generated(2)], None)
        .await
        .unwrap();
    repository
}

#[tokio::test]
async fn replay_validation_fails_on_the_legacy_event_without_the_upcaster() {
    let repository = seeded_repository().await;

    let stream = repository.stream_all_events::<Nonce>().await.unwrap();
    let error = validate_event_stream::<Nonce>(stream, &[]).await.unwrap_err();

    // The error names the aggregate, reports how many events validated before the failure, and
    // wraps the underlying deserialization error -- exactly what gets logged and served by
    // `/readyz` when a deploy is missing an upcaster.
    assert_eq!(error.aggregate_type, Nonce::TYPE);
    assert_eq!(error.validated_count, 0);
    assert!(matches!(error.source, PersistenceError::DeserializationError(_)));
    assert!(error
        .to_string()
        .starts_with("event replay validation failed for aggregate type `nonce`"));
}

#[tokio::test]
async fn replay_validation_passes_once_the_upcaster_is_registered() {
    let repository = seeded_repository().await;

    let stream = repository.stream_all_events::<Nonce>().await.unwrap();
    let validated_count = validate_event_stream::<Nonce>(stream, &[demo_upcaster()])
        .await
        .unwrap();

    assert_eq!(validated_count, 2);
}
