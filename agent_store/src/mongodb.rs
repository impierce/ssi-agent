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
    build_cloud_event, CloudEvent, EventBusError, EventFilter, EventHistoryReader, EventSource, EventSourceStream,
    HistoryAscendingResult, Position, SourceEvent, SubscribePosition,
};
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;
use tokio_stream::StreamExt;

impl<A> AggregateHandler<A, PersistedEventStore<MongoEventRepository, A>>
where
    A: Aggregate,
{
    async fn new(client: Client, services: A::Services) -> Self {
        let repo = MongoEventRepository::new(client)
            .await
            .expect("Failed to create MongoEventRepository");
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
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

    pub async fn verify_events(&self) -> Result<EventVerificationReport, EventVerificationError> {
        self.verify_events_with(event_verification::core_event_verifiers())
            .await
    }

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
        event_publishers: Vec<Box<dyn Query<A>>>,
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
                    .with_parameters(
                        aggregate.clone(),
                        all_aggregates.clone(),
                        event_publishers,
                        &all_aggregates_name,
                    ),
            ),
            aggregate,
            all_aggregates,
        )
    }
}

/// Converts a raw MongoDB BSON document from the `events` collection into a standard [`CloudEvent`].
pub fn document_to_cloud_event(document: &bson::Document) -> Option<CloudEvent> {
    let aggregate_type = document.get_str("aggregate_type").ok()?;
    let aggregate_id = document.get_str("aggregate_id").ok()?;
    let sequence = document.get_i64("sequence").ok()? as usize;
    let event_type = document.get_str("event_type").ok()?;
    let payload_bson = document.get("payload")?;
    let payload: serde_json::Value = bson::from_bson(payload_bson.clone()).ok()?;
    let metadata_doc = document.get_document("metadata").ok();

    let occurred_at = metadata_doc
        .as_ref()
        .and_then(|metadata| metadata.get_str("timestamp").ok())
        .and_then(|timestamp_str| chrono::DateTime::parse_from_rfc3339(timestamp_str).ok())
        .map(|parsed_datetime| parsed_datetime.with_timezone(&chrono::Utc));

    Some(build_cloud_event(
        aggregate_type,
        aggregate_id,
        sequence,
        event_type,
        payload,
        occurred_at,
    ))
}

/// An [`EventSource`] implementation for MongoDB using change streams.
#[derive(Clone)]
pub struct MongoEventSource {
    client: Client,
}

impl MongoEventSource {
    pub fn new(client: Client) -> Self {
        Self { client }
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

        let _ = database.create_collection("events").await;
        let collection = database.collection::<bson::Document>("events");

        let mut options = mongodb::options::ChangeStreamOptions::default();
        if let SubscribePosition::From(ref pos) = from {
            // Reconnect using the serialized BSON ResumeToken so MongoDB replays all changes
            // starting immediately after the last successfully acknowledged event.
            if let Ok(resume_token) = bson::from_slice::<mongodb::change_stream::event::ResumeToken>(&pos.0) {
                options.resume_after = Some(resume_token);
            }
        }

        let change_stream = collection
            .watch()
            .with_options(options)
            .await
            .map_err(|error| EventBusError::Source(error.to_string()))?;

        let stream = change_stream.filter_map(|change_result| match change_result {
            Ok(change) => {
                let Some(document) = change.full_document else {
                    tracing::warn!("Change stream event missing full_document");
                    return None;
                };

                let position = bson::to_vec(&change.id).ok().map(Position);

                match document_to_cloud_event(&document) {
                    Some(cloud_event) => Some(Ok(SourceEvent::new(cloud_event, position))),
                    None => {
                        tracing::warn!("Failed to convert change stream document to CloudEvent");
                        None
                    }
                }
            }
            Err(error) => Some(Err(EventBusError::Source(error.to_string()))),
        });

        Ok(Box::pin(stream))
    }
}

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

        let collection = database.collection::<bson::Document>("events");

        let mut gap_detected = false;
        let mut target_object_id: Option<mongodb::bson::oid::ObjectId> = None;

        let mut base_query = doc! {};
        if !filter.sources.is_empty() {
            let aggregate_types: Vec<String> = filter
                .sources
                .iter()
                .map(|source| source.trim_start_matches("/services/").trim_matches('/').to_lowercase())
                .collect();
            if aggregate_types.len() == 1 {
                base_query.insert("aggregate_type", &aggregate_types[0]);
            } else if let Ok(bson_array) = bson::to_bson(&aggregate_types) {
                base_query.insert("aggregate_type", doc! { "$in": bson_array });
            }
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
            return Ok(HistoryAscendingResult { events, gap_detected });
        }

        if let Some(target_id) = target_object_id {
            // Case A: Resume from target_id: scan index in ascending order for matching documents created after target_id.
            let mut query = base_query;
            query.insert("_id", doc! { "$gt": target_id });
            let find_options = mongodb::options::FindOptions::builder().sort(doc! { "_id": 1 }).build();

            let mut cursor = collection
                .find(query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;
                if let Some(cloud_event) = document_to_cloud_event(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                        if let Some(max_limit) = limit {
                            if events.len() >= max_limit {
                                break;
                            }
                        }
                    }
                }
            }
        } else if let Some(max_limit) = limit {
            // Case B: No last_event_id, but a limit is requested. Fetch the latest matching documents using descending sort,
            // then reverse the resulting vector so the consumer receives them in ascending (chronological) order.
            let find_options = mongodb::options::FindOptions::builder()
                .sort(doc! { "_id": -1 })
                .build();

            let mut cursor = collection
                .find(base_query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;
                if let Some(cloud_event) = document_to_cloud_event(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                        if events.len() >= max_limit {
                            break;
                        }
                    }
                }
            }

            events.reverse();
        } else {
            // Case C: Unbounded query without last_event_id: scan matching documents in natural chronological order.
            let find_options = mongodb::options::FindOptions::builder().sort(doc! { "_id": 1 }).build();

            let mut cursor = collection
                .find(base_query)
                .with_options(find_options)
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?;

            while cursor
                .advance()
                .await
                .map_err(|error| EventBusError::Source(error.to_string()))?
            {
                let document = cursor
                    .deserialize_current()
                    .map_err(|error| EventBusError::Source(error.to_string()))?;
                if let Some(cloud_event) = document_to_cloud_event(&document) {
                    if filter.matches(&cloud_event) {
                        events.push(cloud_event);
                    }
                }
            }
        }

        Ok(HistoryAscendingResult { events, gap_detected })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_to_cloud_event_with_timestamp() {
        let doc = doc! {
            "aggregate_type": "client",
            "aggregate_id": "unime",
            "sequence": 1i64,
            "event_type": "ClientRegistered",
            "payload": {
                "ClientRegistered": {
                    "client_id": "unime"
                }
            },
            "metadata": {
                "timestamp": "2026-09-21T07:50:45.686998501Z"
            }
        };

        let cloud_event = document_to_cloud_event(&doc).expect("Should convert to CloudEvent");
        assert_eq!(cloud_event.id, "client:unime:1");
        assert_eq!(cloud_event.source, "/services/client");
        assert_eq!(cloud_event.event_type, "com.impierce.unicore.client-registered");
        assert_eq!(
            cloud_event.time.unwrap().to_rfc3339(),
            "2026-09-21T07:50:45.686998501+00:00"
        );
    }

    #[test]
    fn test_document_to_cloud_event_with_did_id() {
        let doc = doc! {
            "aggregate_type": "document",
            "aggregate_id": "did:key:zDnaek2KMsYPpaxWo3c49AqqCVDSJTFra9Mj9FpfuYSKKBaQs",
            "sequence": 6i64,
            "event_type": "PublicKeyUpdated",
            "payload": {
                "PublicKeyUpdated": {}
            },
            "metadata": {
                "timestamp": "2026-09-21T12:39:09.732083608Z"
            }
        };

        let cloud_event = document_to_cloud_event(&doc).expect("Should convert to CloudEvent");
        assert_eq!(
            cloud_event.id,
            "document:did:key:zDnaek2KMsYPpaxWo3c49AqqCVDSJTFra9Mj9FpfuYSKKBaQs:6"
        );

        // Verify split logic matches correctly
        let (aggregate_type, rest) = cloud_event.id.split_once(':').unwrap();
        let (aggregate_id, sequence_str) = rest.rsplit_once(':').unwrap();
        assert_eq!(aggregate_type, "document");
        assert_eq!(
            aggregate_id,
            "did:key:zDnaek2KMsYPpaxWo3c49AqqCVDSJTFra9Mj9FpfuYSKKBaQs"
        );
        assert_eq!(sequence_str, "6");
    }
}
