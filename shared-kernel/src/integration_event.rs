use crate::event_bus::CloudEvent;
use cqrs_es::{Aggregate, EventEnvelope};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;

/// A formal Integration Event in the Published Language (PL).
///
/// Unlike internal CQRS domain events which represent private aggregate state changes,
/// integration events establish a stable contract for external consumers,
/// webhook subscribers, and cross-system streaming.
pub trait IntegrationEvent: Serialize + Send + Sync + 'static {
    /// [`CloudEvent`] `source` URI for all variants of this integration event enum (e.g. `"/services/issuance"`).
    const SOURCE: &'static str;

    /// Reverse-DNS event type, e.g. `com.impierce.unicore.issuance.credential.offered`.
    fn event_type(&self) -> &'static str;

    /// Optional entity or aggregate subject ID.
    fn subject(&self) -> Option<String>;

    /// Converts this integration event into a CNCF [`CloudEvent`] v1.0.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization of `self` into a JSON value fails.
    fn into_cloud_event(
        self,
        source: &str,
        caller_id: Option<String>,
        caller_type: Option<String>,
    ) -> Result<CloudEvent, serde_json::Error>
    where
        Self: Sized,
    {
        let data = serde_json::to_value(&self)?;
        let subject = self.subject();
        let event_type = self.event_type();
        let mut ce = CloudEvent::new(event_type, source)
            .with_data(data)
            .with_caller(caller_id, caller_type);
        if let Some(s) = subject {
            ce = ce.with_subject(s);
        }
        Ok(ce)
    }

    /// Projects this integration event into a CNCF [`CloudEvent`] v1.0 preserving the originating
    /// [`EventEnvelope<A>`]'s deterministic `id` (`{A::TYPE}:{aggregate_id}:{sequence}`),
    /// `timestamp`, and caller provenance (`callerid`, `callertype`).
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if serialization of `self` into a JSON value fails.
    fn into_projected_cloud_event<A: Aggregate>(
        self,
        envelope: &EventEnvelope<A>,
        source: &str,
    ) -> Result<CloudEvent, serde_json::Error>
    where
        Self: Sized,
    {
        let caller_id = envelope.metadata.get("callerid").filter(|s| !s.is_empty()).cloned();
        let caller_type = envelope.metadata.get("callertype").filter(|s| !s.is_empty()).cloned();
        let mut ce = self.into_cloud_event(source, caller_id, caller_type)?;
        ce.id = format!("{}:{}:{}", A::TYPE, envelope.aggregate_id, envelope.sequence);
        ce.time = envelope
            .metadata
            .get("timestamp")
            .and_then(|timestamp_str| chrono::DateTime::parse_from_rfc3339(timestamp_str).ok())
            .map(|datetime| datetime.with_timezone(&chrono::Utc));
        Ok(ce)
    }
}

/// Implemented by [`Aggregate`]s whose domain events project into public [`IntegrationEvent`]s.
///
/// Aggregates that only emit private internal domain events do not implement this trait.
pub trait ProjectIntegrationEvent: Aggregate {
    /// The public [`IntegrationEvent`] enum that this aggregate's domain events project into.
    type IntegrationEvent: IntegrationEvent;

    /// Projects a domain [`EventEnvelope`] for this aggregate into a public [`CloudEvent`],
    /// returning `None` if the domain event has no public integration event mapping.
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent>;
}

/// Type-erased descriptor for projecting persisted domain events of a specific [`Aggregate`] into
/// public [`CloudEvent`]s when reading from a persistent store (such as `MongoDB` Change Streams
/// or historical catch-up queries).
#[derive(Clone, Copy)]
pub struct EventProjector {
    aggregate_type: &'static str,
    source: &'static str,
    project: fn(&str, usize, serde_json::Value, HashMap<String, String>) -> Option<CloudEvent>,
}

impl EventProjector {
    #[must_use]
    pub const fn for_aggregate<A>() -> Self
    where
        A: ProjectIntegrationEvent,
        A::Event: DeserializeOwned,
    {
        Self {
            aggregate_type: A::TYPE,
            source: <A::IntegrationEvent as IntegrationEvent>::SOURCE,
            project: project_stored_event::<A>,
        }
    }

    #[must_use]
    pub const fn aggregate_type(&self) -> &'static str {
        self.aggregate_type
    }

    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.source
    }

    #[must_use]
    pub fn project(
        &self,
        aggregate_id: &str,
        sequence: usize,
        payload: serde_json::Value,
        metadata: HashMap<String, String>,
    ) -> Option<CloudEvent> {
        (self.project)(aggregate_id, sequence, payload, metadata)
    }
}

fn project_stored_event<A>(
    aggregate_id: &str,
    sequence: usize,
    payload: serde_json::Value,
    metadata: HashMap<String, String>,
) -> Option<CloudEvent>
where
    A: ProjectIntegrationEvent,
    A::Event: DeserializeOwned,
{
    let payload: A::Event = serde_json::from_value(payload).ok()?;
    let envelope = EventEnvelope {
        aggregate_id: aggregate_id.to_string(),
        sequence,
        payload,
        metadata,
    };
    A::project_integration_event(&envelope)
}
