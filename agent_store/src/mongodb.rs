use crate::event_verification::{self, EventVerificationError, EventVerificationReport, EventVerifier, RawStoredEvent};
use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::{application_state::Command, config::config};
use async_trait::async_trait;
use cqrs_es::{persist::PersistedEventStore, CqrsFramework};
use cqrs_es::{Aggregate, Query, View};
use mongo_es::{default_mongo_client, Client, MongoEventRepository, MongoViewRepository};
use mongodb::bson::{self, doc, Document};
use mongodb::options::FindOptions;
use shared_kernel::event_bus::{
    CloudEvent, EventBusError, EventFilter, EventHistoryReader, EventSource, EventSourceStream, HistoryAscendingResult,
    Position, SourceEvent, SubscribePosition,
};
use shared_kernel::integration_event::EventProjector;
use shared_kernel::view_repository::DynViewRepository;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_stream::StreamExt;

impl<A> AggregateHandler<A, PersistedEventStore<MongoEventRepository, A>>
where
    A: Aggregate,
{
    async fn new(client: Client, services: A::Services) -> Self {
        let repo = new_event_repository(client).await;
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
        }
    }
}

/// Builds the event repository, retrying while MongoDB is still coming up.
///
/// A deployment scheduled alongside its database would otherwise panic before MongoDB accepts
/// connections. A genuine misconfiguration still panics, once the retries are exhausted.
async fn new_event_repository(client: Client) -> MongoEventRepository {
    const MAX_ATTEMPTS: u32 = 5;

    let mut backoff = std::time::Duration::from_millis(500);
    let mut attempt = 1;
    loop {
        match MongoEventRepository::new(client.clone()).await {
            Ok(repository) => return repository,
            Err(error) => {
                assert!(
                    attempt < MAX_ATTEMPTS,
                    "Failed to create MongoEventRepository after {MAX_ATTEMPTS} attempts: {error:?}"
                );
                tracing::warn!(
                    "Failed to create MongoEventRepository (attempt {attempt}), retrying in {backoff:?}: {error:?}"
                );
                tokio::time::sleep(backoff).await;
                backoff *= 2;
                attempt += 1;
            }
        }
    }
}

pub struct MongoDB {
    pub client: Client,
}

impl MongoDB {
    pub async fn new() -> Self {
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let client = default_mongo_client(&connection_string).await;
        Self { client }
    }
    // TODO: Run [Client::shutdown] during graceful shutdown to close all open connections.

    pub async fn verify_events_with(
        &self,
        verifiers: &[EventVerifier],
    ) -> Result<EventVerificationReport, EventVerificationError> {
        Ok(event_verification::verify_events_with(
            self.load_raw_events().await?,
            verifiers,
        ))
    }

    async fn load_raw_events(&self) -> Result<Vec<RawStoredEvent>, EventVerificationError> {
        #[derive(serde::Deserialize)]
        struct RawStoredEventDocument {
            aggregate_type: String,
            aggregate_id: String,
            sequence: i64,
            event_type: String,
            event_version: String,
            payload: serde_json::Value,
        }

        impl From<RawStoredEventDocument> for RawStoredEvent {
            fn from(event: RawStoredEventDocument) -> Self {
                Self {
                    aggregate_type: event.aggregate_type,
                    aggregate_id: event.aggregate_id,
                    sequence: event.sequence,
                    event_type: event.event_type,
                    event_version: event.event_version,
                    payload: event.payload,
                }
            }
        }

        let Some(database) = self.client.default_database() else {
            return Err(EventVerificationError::MissingMongoDefaultDatabase);
        };

        let collection = database.collection::<Document>("events");
        let options = FindOptions::builder()
            .sort(doc! { "aggregate_type": 1, "aggregate_id": 1, "sequence": 1 })
            .build();

        let mut cursor = collection.find(doc! {}).with_options(options).await?;
        let mut events = Vec::new();

        while cursor.advance().await? {
            let document = cursor.deserialize_current()?;
            let event: RawStoredEventDocument = bson::from_document(document)?;
            events.push(event.into());
        }

        Ok(events)
    }
}

impl CqrsComponentBuilder for MongoDB {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn DynViewRepository<V, A>>,
        Arc<dyn DynViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::TYPE);

        // Initialize the MongoDB repositories.
        let aggregate: Arc<MongoViewRepository<V, A>> =
            Arc::new(MongoViewRepository::new(A::TYPE, self.client.clone()));
        let all_aggregates: Arc<MongoViewRepository<AV, A>> =
            Arc::new(MongoViewRepository::new(&all_aggregates_name, self.client.clone()));

        (
            Arc::new(
                AggregateHandler::new(self.client.clone(), services)
                    .await
                    .with_parameters(aggregate.clone(), all_aggregates.clone(), queries, &all_aggregates_name),
            ),
            aggregate,
            all_aggregates,
        )
    }
}

/// Projects a raw MongoDB BSON document from the `events` collection into a [`CloudEvent`]
/// using the registered [`EventProjector`]s.
///
/// Returns `None` if the document's `aggregate_type` has no registered integration event projector,
/// if the domain event variant does not map to a public integration event, or if the BSON document
/// is malformed.
#[must_use]
pub fn document_to_cloud_event(document: &bson::Document, projectors: &[EventProjector]) -> Option<CloudEvent> {
    let aggregate_type = document.get_str("aggregate_type").ok()?;
    let projector = projectors.iter().find(|p| p.aggregate_type() == aggregate_type)?;
    let aggregate_id = document.get_str("aggregate_id").ok()?;
    let sequence = usize::try_from(document.get_i64("sequence").ok()?).ok()?;
    let payload_bson = document.get("payload")?;
    let payload: serde_json::Value = bson::from_bson(payload_bson.clone()).ok()?;
    let metadata: HashMap<String, String> = document
        .get_document("metadata")
        .ok()
        .and_then(|doc| bson::from_bson(bson::Bson::Document(doc.clone())).ok())
        .unwrap_or_default();

    projector.project(aggregate_id, sequence, payload, metadata)
}

/// An [`EventSource`] and [`EventHistoryReader`] implementation for MongoDB using change streams.
#[derive(Clone)]
pub struct MongoEventSource {
    client: Client,
    initialized: Arc<tokio::sync::OnceCell<()>>,
    projectors: Arc<std::sync::RwLock<Vec<EventProjector>>>,
}

impl MongoEventSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            initialized: Arc::new(tokio::sync::OnceCell::new()),
            projectors: Arc::new(std::sync::RwLock::new(Vec::new())),
        }
    }

    /// Configures the initial set of [`EventProjector`]s applied to stored [`cqrs_es::EventEnvelope`]s.
    #[must_use]
    pub fn with_projectors(self, projectors: &[EventProjector]) -> Self {
        self.register_projectors(projectors);
        self
    }

    /// Registers additional [`EventProjector`]s (e.g., from extension bounded contexts).
    pub fn register_projectors(&self, projectors: &[EventProjector]) {
        let mut guard = match self.projectors.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.extend_from_slice(projectors);
    }

    fn convert_document(&self, document: &bson::Document) -> Option<CloudEvent> {
        let guard = match self.projectors.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        document_to_cloud_event(document, &guard)
    }

    /// Creates the `events` collection and the index backing `aggregate_type` catch-up queries, once per process.
    ///
    /// `mongo-es` indexes only `{aggregate_id, sequence}` and `{aggregate_id, current_snapshot}`,
    /// leaving the reader's `aggregate_type` filter with nothing to use.
    async fn ensure_initialized(&self, database: &mongodb::Database) {
        self.initialized
            .get_or_init(|| async {
                // Errors when the collection already exists, which is the steady state.
                let _ = database.create_collection("events").await;

                // Spawned, not awaited: `createIndexes` returns only once the build completes, and
                // this runs inside the SSE request path. Queries run unindexed until it lands.
                let collection = database.collection::<bson::Document>("events");
                tokio::spawn(async move {
                    if let Err(error) = collection
                        .create_index(
                            mongodb::IndexModel::builder()
                                .keys(doc! { "aggregate_type": 1, "_id": 1 })
                                .build(),
                        )
                        .await
                    {
                        tracing::warn!(
                            "Failed to create the events index; queries filtering on `aggregate_type` remain unindexed: {error}"
                        );
                    }
                });
            })
            .await;
    }
}

#[async_trait]
impl EventSource for MongoEventSource {
    /// Opens a change-stream listener on the MongoDB `events` collection.
    ///
    /// Supports resuming from a specific position when `SubscribePosition::From` contains a valid serialized BSON [`ResumeToken`].
    async fn open(&self, from: SubscribePosition) -> Result<EventSourceStream, EventBusError> {
        let database = self
            .client
            .default_database()
            .ok_or_else(|| EventBusError::Source("No default database configured on MongoDB client".to_string()))?;

        self.ensure_initialized(&database).await;
        let collection = database.collection::<bson::Document>("events");

        let mut options = mongodb::options::ChangeStreamOptions::default();
        if let SubscribePosition::From(ref pos) = from {
            // Reconnect using the serialized BSON ResumeToken so MongoDB replays all changes
            // starting immediately after the last successfully acknowledged event.
            //
            // Reported rather than ignored: falling through to a live stream would silently skip
            // every event since the checkpoint.
            let resume_token = bson::from_slice::<mongodb::change_stream::event::ResumeToken>(&pos.0)
                .map_err(|_| EventBusError::UnsupportedPosition)?;
            options.resume_after = Some(resume_token);
        }

        let change_stream = collection
            .watch()
            .with_options(options)
            .await
            .map_err(|error| EventBusError::Source(error.to_string()))?;

        let source = self.clone();
        let stream = change_stream.filter_map(move |change_result| match change_result {
            Ok(change) => {
                let Some(document) = change.full_document else {
                    tracing::warn!("Change stream event missing full_document");
                    return None;
                };

                let position = bson::to_vec(&change.id).ok().map(Position);
                // Unmapped internal domain events return `None` here; `position` advances on the next
                // projected integration event (any unmapped events re-scanned on reconnect remain no-ops).
                let cloud_event = source.convert_document(&document)?;
                Some(Ok(SourceEvent::new(cloud_event, position)))
            }
            Err(error) => Some(Err(EventBusError::Source(error.to_string()))),
        });

        Ok(Box::pin(stream))
    }
}

/// Hard ceiling on documents read from MongoDB for a single catch-up query.
///
/// `event_types`, `since` and `until` are applied in Rust after reading, so without this cap a
/// highly selective filter degrades into a full scan.
const MAX_DOCUMENTS_EXAMINED: usize = 10_000;

#[async_trait]
impl EventHistoryReader for MongoEventSource {
    async fn history_ascending(
        &self,
        filter: &EventFilter,
        last_event_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<HistoryAscendingResult, EventBusError> {
        let database = self
            .client
            .default_database()
            .ok_or_else(|| EventBusError::Source("No default database configured on MongoDB client".to_string()))?;

        self.ensure_initialized(&database).await;
        let collection = database.collection::<bson::Document>("events");

        let mut gap_detected = false;
        let mut truncated = false;
        let mut resume_after: Option<String> = None;
        let mut target_object_id: Option<mongodb::bson::oid::ObjectId> = None;

        let mut base_query = doc! {};
        let registered_aggregate_types: Vec<&str> = {
            let guard = match self.projectors.read() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.iter().map(EventProjector::aggregate_type).collect()
        };
        if !registered_aggregate_types.is_empty() {
            base_query.insert("aggregate_type", doc! { "$in": registered_aggregate_types });
        }
        if let Some(ref target_subject) = filter.subject {
            base_query.insert("aggregate_id", target_subject);
        }

        // 1. If last_event_id is provided ("aggregate_type:aggregate_id:sequence"), find its MongoDB _id (ObjectId).
        // Once resolved, we can stream subsequent events in exact chronological order via `{ _id: { $gt: target_id } }`.
        if let Some(last_id) = last_event_id {
            if let Some((aggregate_type, rest)) = last_id.split_once(':') {
                if let Some((aggregate_id, sequence_str)) = rest.rsplit_once(':') {
                    if let Ok(sequence_num) = sequence_str.parse::<i64>() {
                        let reference_query = doc! {
                            "aggregate_type": aggregate_type,
                            "aggregate_id": aggregate_id,
                            "sequence": sequence_num,
                        };
                        let reference_doc = collection
                            .find_one(reference_query)
                            .await
                            .map_err(|error| EventBusError::Source(error.to_string()))?;
                        if let Some(doc) = reference_doc {
                            target_object_id = doc.get_object_id("_id").ok();
                        }
                    }
                }
            }
            // If the specified event cannot be found in MongoDB, signal gap_detected to warn the subscriber.
            if target_object_id.is_none() {
                gap_detected = true;
            }
        }

        let mut events = Vec::new();

        if limit == Some(0) {
            return Ok(HistoryAscendingResult {
                events,
                gap_detected,
                truncated,
                resume_after,
            });
        }

        if let Some(target_id) = target_object_id {
            // Case A: Resume from target_id: scan index in ascending order for matching documents created after target_id.
            let mut query = base_query;
            query.insert("_id", doc! { "$gt": target_id });

            let scan_limit = MAX_DOCUMENTS_EXAMINED;
            let find_options = FindOptions::builder()
                .sort(doc! { "_id": 1 })
                .limit(scan_limit as i64)
                .build();

            let mut cursor = collection
                .find(query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            let mut examined = 0usize;
            let mut last_examined_id: Option<String> = None;
            let mut stopped_on_limit = false;
            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                examined += 1;
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;

                // Raw fields rather than the converted event: a document that fails to convert
                // still advances the scan, and the cursor has to move past it.
                if let (Ok(aggregate_type), Ok(aggregate_id), Ok(sequence)) = (
                    document.get_str("aggregate_type"),
                    document.get_str("aggregate_id"),
                    document.get_i64("sequence"),
                ) {
                    last_examined_id = Some(format!("{aggregate_type}:{aggregate_id}:{sequence}"));
                }

                if let Some(cloud_event) = self.convert_document(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                        if let Some(max_limit) = limit {
                            if events.len() > max_limit {
                                events.truncate(max_limit);
                                truncated = true;
                                stopped_on_limit = true;
                                break;
                            }
                        }
                    }
                }
            }

            if examined >= scan_limit {
                truncated = true;
            }

            if truncated {
                resume_after = if stopped_on_limit {
                    // The document that stopped the scan was dropped from the response, so the
                    // client must resume from the last event it actually received.
                    events.last().map(|event| event.id.clone())
                } else {
                    last_examined_id
                };
            }
        } else if let Some(max_limit) = limit {
            // Case B: No last_event_id, but a limit is requested. Fetch the latest matching documents using descending sort,
            // then reverse the resulting vector so the consumer receives them in ascending (chronological) order.
            let scan_limit = MAX_DOCUMENTS_EXAMINED;
            let find_options = FindOptions::builder()
                .sort(doc! { "_id": -1 })
                .limit(scan_limit as i64)
                .build();

            let mut cursor = collection
                .find(base_query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            let mut examined = 0usize;
            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                examined += 1;
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;
                if let Some(cloud_event) = self.convert_document(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                        if events.len() >= max_limit {
                            break;
                        }
                    }
                }
            }

            // A short result is only a gap when the bounded scan is what cut it short.
            if events.len() < max_limit && examined >= scan_limit {
                truncated = true;
            }

            events.reverse();
        } else {
            // Case C: Unbounded query without last_event_id: scan matching documents in natural chronological order.
            let find_options = FindOptions::builder()
                .sort(doc! { "_id": 1 })
                .limit(MAX_DOCUMENTS_EXAMINED as i64)
                .build();

            let mut cursor = collection
                .find(base_query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            let mut examined = 0usize;
            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                examined += 1;
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;
                if let Some(cloud_event) = self.convert_document(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                    }
                }
            }

            if examined >= MAX_DOCUMENTS_EXAMINED {
                truncated = true;
            }
        }

        Ok(HistoryAscendingResult {
            events,
            gap_detected,
            truncated,
            resume_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent, EventEnvelope};
    use serde::{Deserialize, Serialize};
    use shared_kernel::integration_event::{IntegrationEvent, ProjectIntegrationEvent};
    use std::{convert::Infallible, error::Error, fmt};

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    enum TestEvent {
        Created { id: String },
        InternalOnly,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            match self {
                Self::Created { .. } => "Created".to_string(),
                Self::InternalOnly => "InternalOnly".to_string(),
            }
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug)]
    struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "test error")
        }
    }

    impl Error for TestError {}

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "template";

        type Command = Infallible;
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _service: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Serialize)]
    struct TestIntegrationEvent {
        id: String,
    }

    impl IntegrationEvent for TestIntegrationEvent {
        fn event_type(&self) -> &'static str {
            "com.impierce.unicore.template.created"
        }

        fn subject(&self) -> Option<String> {
            Some(self.id.clone())
        }
    }

    impl ProjectIntegrationEvent for TestAggregate {
        fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
            match &envelope.payload {
                TestEvent::Created { id } => TestIntegrationEvent { id: id.clone() }
                    .into_projected_cloud_event(envelope, "/services/template")
                    .ok(),
                TestEvent::InternalOnly => None,
            }
        }
    }

    const PROJECTORS: &[EventProjector] = &[EventProjector::for_aggregate::<TestAggregate>()];

    #[test]
    fn test_document_to_cloud_event_with_timestamp() {
        let doc = doc! {
            "aggregate_type": "template",
            "aggregate_id": "tpl-123",
            "sequence": 1i64,
            "event_type": "Created",
            "payload": {
                "Created": {
                    "id": "tpl-123"
                }
            },
            "metadata": {
                "timestamp": "2026-09-21T07:50:45.686998501Z"
            }
        };

        let ce = document_to_cloud_event(&doc, PROJECTORS).expect("Should project to CloudEvent");
        assert_eq!(ce.id, "template:tpl-123:1");
        assert_eq!(ce.source, "/services/template");
        assert_eq!(ce.event_type, "com.impierce.unicore.template.created");
        assert_eq!(ce.time.unwrap().to_rfc3339(), "2026-09-21T07:50:45.686998501+00:00");
    }

    #[test]
    fn test_document_to_cloud_event_with_caller() {
        let doc = doc! {
            "aggregate_type": "template",
            "aggregate_id": "tpl-123",
            "sequence": 1i64,
            "event_type": "Created",
            "payload": {
                "Created": {
                    "id": "tpl-123"
                }
            },
            "metadata": {
                "timestamp": "2026-09-21T07:50:45.686998501Z",
                "callerid": "alice@example.test",
                "callertype": "user"
            }
        };

        let ce = document_to_cloud_event(&doc, PROJECTORS).expect("Should project to CloudEvent");
        assert_eq!(ce.extension.callerid.as_deref(), Some("alice@example.test"));
        assert_eq!(ce.extension.callertype.as_deref(), Some("user"));
    }

    #[test]
    fn test_document_to_cloud_event_skips_unmapped_events() {
        let doc = doc! {
            "aggregate_type": "template",
            "aggregate_id": "tpl-123",
            "sequence": 2i64,
            "event_type": "InternalOnly",
            "payload": "InternalOnly",
            "metadata": {}
        };

        assert!(document_to_cloud_event(&doc, PROJECTORS).is_none());
    }
}
