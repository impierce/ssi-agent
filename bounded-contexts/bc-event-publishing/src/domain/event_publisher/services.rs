use crate::domain::event_publisher::config::EventPublisherFilter;
use shared_kernel::{
    event_bus::EventFilter,
    integration_event::{collect_projector_docs, EventProjector},
};
use std::{
    collections::HashSet,
    sync::{Arc, PoisonError, RwLock},
};

/// Why a list of event type patterns was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum InvalidEventTypes {
    #[error("At least one event trigger must be specified in `filter.types`")]
    EmptyEvents,
    #[error("Event trigger pattern must not be empty")]
    EmptyEventPattern,
    #[error("Unknown event trigger `{0}`; does not match any registered integration event type")]
    UnknownEventType(String),
}

/// Thread-safe registry of known [`IntegrationEvent`](shared_kernel::IntegrationEvent) type strings
/// supported by the running deployment.
///
/// Initialized from the core integration event projectors at startup and extended when
/// application extensions register additional event projectors.
#[derive(Clone, Debug)]
pub struct KnownEventTypes {
    inner: Arc<RwLock<HashSet<String>>>,
}

impl Default for KnownEventTypes {
    fn default() -> Self {
        Self::empty()
    }
}

impl KnownEventTypes {
    /// Creates an empty `KnownEventTypes` registry.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Creates a `KnownEventTypes` registry populated with the given event type strings.
    pub fn with_event_types<I, S>(types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            inner: Arc::new(RwLock::new(types.into_iter().map(Into::into).collect())),
        }
    }

    /// Creates a `KnownEventTypes` registry populated from a slice of [`EventProjector`]s.
    #[must_use]
    pub fn from_projectors(projectors: &[EventProjector]) -> Self {
        let registry = Self::empty();
        registry.extend_from_projectors(projectors);
        registry
    }

    /// Adds all integration event types documented by `projectors` into this registry.
    pub fn extend_from_projectors(&self, projectors: &[EventProjector]) {
        let docs = collect_projector_docs(projectors);
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        for group in docs {
            for variant in group.variants {
                guard.insert(variant.event_type.to_string());
            }
        }
    }

    /// Validates and normalizes a list of event type patterns against the registered integration events.
    ///
    /// Supports exact event types (`com.impierce.unicore.issuance.credential.issued`), short suffixes
    /// (`issuance.credential.issued`), prefix wildcards (`com.impierce.unicore.issuance.*`), and `*`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidEventTypes`] if `events` is empty, contains a blank entry, or contains a
    /// pattern that does not match any registered integration event type.
    pub fn validate_and_normalize(&self, events: Vec<String>) -> Result<Vec<String>, InvalidEventTypes> {
        if events.is_empty() {
            return Err(InvalidEventTypes::EmptyEvents);
        }

        let known_guard = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        let mut normalized = Vec::with_capacity(events.len());

        for pattern in events {
            let trimmed = pattern.trim().to_string();
            if trimmed.is_empty() {
                return Err(InvalidEventTypes::EmptyEventPattern);
            }

            let filter = EventFilter {
                event_types: vec![trimmed.clone()],
                ..Default::default()
            };

            let matches_known = known_guard.is_empty()
                || known_guard
                    .iter()
                    .any(|event_type| filter.matches_event_type(event_type));

            if !matches_known {
                return Err(InvalidEventTypes::UnknownEventType(trimmed));
            }

            if !normalized.contains(&trimmed) {
                normalized.push(trimmed);
            }
        }

        Ok(normalized)
    }

    /// Validates and normalizes an [`EventPublisherFilter`] against the registered integration events.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidEventTypes`] if `filter.types` is empty, contains a blank entry, or contains a
    /// pattern that does not match any registered integration event type.
    pub fn validate_and_normalize_filter(
        &self,
        filter: EventPublisherFilter,
    ) -> Result<EventPublisherFilter, InvalidEventTypes> {
        Ok(EventPublisherFilter {
            types: self.validate_and_normalize(filter.types)?,
        })
    }
}

/// Domain services for the [`EventPublisher`](super::aggregate::EventPublisher) aggregate.
#[derive(Clone, Debug, Default)]
pub struct EventPublisherDomainServices {
    known_event_types: KnownEventTypes,
}

impl EventPublisherDomainServices {
    #[must_use]
    pub fn new(known_event_types: KnownEventTypes) -> Self {
        Self { known_event_types }
    }

    #[must_use]
    pub fn known_event_types(&self) -> &KnownEventTypes {
        &self.known_event_types
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_rwlock_still_enforces_known_event_types() {
        let known = KnownEventTypes::with_event_types(["com.impierce.unicore.issuance.credential.issued"]);
        let poisoned = known.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.inner.write().unwrap();
            panic!("intentional panic to poison RwLock");
        })
        .join();

        assert!(known.inner.read().is_err());

        assert_eq!(
            known
                .validate_and_normalize(vec!["issuance.credential.issued".to_string()])
                .unwrap(),
            vec!["issuance.credential.issued".to_string()]
        );
        assert_eq!(
            known
                .validate_and_normalize(vec!["com.impierce.unicore.unknown.event".to_string()])
                .unwrap_err(),
            InvalidEventTypes::UnknownEventType("com.impierce.unicore.unknown.event".to_string())
        );

        known.extend_from_projectors(&[]);
    }
}
