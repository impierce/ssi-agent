use crate::domain::event_publisher::config::EventPublisherFilter;
use shared_kernel::{event_bus::EventFilter, integration_event::EventProjector};
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
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        for projector in projectors {
            for variant in projector.docs().variants {
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
#[derive(Clone, Debug)]
pub struct EventPublisherDomainServices {
    known_event_types: KnownEventTypes,
    allow_local_network_targets: bool,
}

impl Default for EventPublisherDomainServices {
    fn default() -> Self {
        Self::new(KnownEventTypes::default())
    }
}

impl EventPublisherDomainServices {
    #[must_use]
    pub fn new(known_event_types: KnownEventTypes) -> Self {
        Self {
            known_event_types,
            allow_local_network_targets: cfg!(feature = "allow-localhost"),
        }
    }

    pub(crate) fn with_allow_local_network_targets(mut self, allow: bool) -> Self {
        self.allow_local_network_targets = allow;
        self
    }

    #[must_use]
    pub fn known_event_types(&self) -> &KnownEventTypes {
        &self.known_event_types
    }

    #[must_use]
    pub fn allow_local_network_targets(&self) -> bool {
        self.allow_local_network_targets
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

    mod duplicate_enum_name_fixtures {
        use serde::{Deserialize, Serialize};
        use shared_kernel::{
            cqrs_es::{event_sink::EventSink, Aggregate, DomainEvent, EventEnvelope},
            event_bus::CloudEvent,
            integration_event::{
                IntegrationEvent, IntegrationEventGroupDoc, IntegrationEventVariantDoc, ProjectIntegrationEvent,
            },
        };

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        pub struct DummyEvent;
        impl DomainEvent for DummyEvent {
            fn event_type(&self) -> String {
                "DummyEvent".to_string()
            }
            fn event_version(&self) -> String {
                "1.0".to_string()
            }
        }

        #[derive(Debug, thiserror::Error)]
        #[error("dummy")]
        pub struct DummyError;

        pub mod first {
            use super::{
                Aggregate, CloudEvent, Deserialize, DummyError, DummyEvent, EventEnvelope, EventSink, IntegrationEvent,
                IntegrationEventGroupDoc, IntegrationEventVariantDoc, ProjectIntegrationEvent, Serialize,
            };

            #[derive(Serialize)]
            pub enum SharedNameEvents {}

            impl IntegrationEvent for SharedNameEvents {
                const SOURCE: &'static str = "/services/first";
                const DOCS: IntegrationEventGroupDoc = IntegrationEventGroupDoc {
                    name: "SharedNameEvents",
                    source: Self::SOURCE,
                    description: "",
                    variants: &[IntegrationEventVariantDoc {
                        variant_name: "Alpha",
                        event_type: "com.impierce.unicore.first.alpha",
                        description: "",
                        subject_field: None,
                        fields: &[],
                    }],
                };
                fn event_type(&self) -> &'static str {
                    match *self {}
                }
                fn subject(&self) -> Option<String> {
                    match *self {}
                }
            }

            #[derive(Default, Serialize, Deserialize)]
            pub struct FirstAggregate;
            impl Aggregate for FirstAggregate {
                const TYPE: &'static str = "first";
                type Command = ();
                type Event = DummyEvent;
                type Error = DummyError;
                type Services = ();
                fn handle(
                    &mut self,
                    (): (),
                    (): &(),
                    _: &EventSink<Self>,
                ) -> impl std::future::Future<Output = Result<(), DummyError>> {
                    std::future::ready(Ok(()))
                }
                fn apply(&mut self, _: DummyEvent) {}
            }
            impl ProjectIntegrationEvent for FirstAggregate {
                type IntegrationEvent = SharedNameEvents;
                fn project_integration_event(_: &EventEnvelope<Self>) -> Option<CloudEvent> {
                    None
                }
            }
        }

        pub mod second {
            use super::{
                Aggregate, CloudEvent, Deserialize, DummyError, DummyEvent, EventEnvelope, EventSink, IntegrationEvent,
                IntegrationEventGroupDoc, IntegrationEventVariantDoc, ProjectIntegrationEvent, Serialize,
            };

            #[derive(Serialize)]
            pub enum SharedNameEvents {}

            impl IntegrationEvent for SharedNameEvents {
                const SOURCE: &'static str = "/services/second";
                const DOCS: IntegrationEventGroupDoc = IntegrationEventGroupDoc {
                    name: "SharedNameEvents",
                    source: Self::SOURCE,
                    description: "",
                    variants: &[IntegrationEventVariantDoc {
                        variant_name: "Beta",
                        event_type: "com.impierce.unicore.second.beta",
                        description: "",
                        subject_field: None,
                        fields: &[],
                    }],
                };
                fn event_type(&self) -> &'static str {
                    match *self {}
                }
                fn subject(&self) -> Option<String> {
                    match *self {}
                }
            }

            #[derive(Default, Serialize, Deserialize)]
            pub struct SecondAggregate;
            impl Aggregate for SecondAggregate {
                const TYPE: &'static str = "second";
                type Command = ();
                type Event = DummyEvent;
                type Error = DummyError;
                type Services = ();
                fn handle(
                    &mut self,
                    (): (),
                    (): &(),
                    _: &EventSink<Self>,
                ) -> impl std::future::Future<Output = Result<(), DummyError>> {
                    std::future::ready(Ok(()))
                }
                fn apply(&mut self, _: DummyEvent) {}
            }
            impl ProjectIntegrationEvent for SecondAggregate {
                type IntegrationEvent = SharedNameEvents;
                fn project_integration_event(_: &EventEnvelope<Self>) -> Option<CloudEvent> {
                    None
                }
            }
        }
    }

    #[test]
    fn extend_from_projectors_keeps_variants_when_two_enums_share_the_same_name() {
        use duplicate_enum_name_fixtures as fixtures;
        use shared_kernel::integration_event::EventProjector;

        let projectors = [
            EventProjector::for_aggregate::<fixtures::first::FirstAggregate>(),
            EventProjector::for_aggregate::<fixtures::second::SecondAggregate>(),
        ];
        let known = KnownEventTypes::from_projectors(&projectors);
        assert_eq!(
            known
                .validate_and_normalize(vec![
                    "com.impierce.unicore.first.alpha".to_string(),
                    "com.impierce.unicore.second.beta".to_string(),
                ])
                .unwrap(),
            vec![
                "com.impierce.unicore.first.alpha".to_string(),
                "com.impierce.unicore.second.beta".to_string(),
            ]
        );
    }
}
