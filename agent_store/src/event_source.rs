use async_trait::async_trait;
use mongo_es::Client;
use mongodb::bson::{self, doc};
use shared_kernel::event_bus::{
    build_cloud_event, BusEventStream, CloudEvent, EventBusError, EventFilter, EventHistoryReader, EventSource,
    HistoryAscendingResult, SubscribePosition,
};
use tokio_stream::StreamExt;

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
    async fn open(&self, from: SubscribePosition) -> Result<BusEventStream, EventBusError> {
        let database = self
            .client
            .default_database()
            .ok_or_else(|| EventBusError::Source("No default database configured on MongoDB client".to_string()))?;

        let _ = database.create_collection("events").await;
        let collection = database.collection::<bson::Document>("events");

        let mut options = mongodb::options::ChangeStreamOptions::default();
        if let SubscribePosition::From(ref pos) = from {
            // Deserialize resume token from position bytes if available
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

                match document_to_cloud_event(&document) {
                    Some(cloud_event) => Some(Ok(cloud_event)),
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

        if let Some(last_id) = last_event_id {
            if let Some((aggregate_type, rest)) = last_id.split_once(':') {
                if let Some((aggregate_id, sequence_str)) = rest.rsplit_once(':') {
                    if let Ok(sequence_num) = sequence_str.parse::<i64>() {
                        let reference_query = doc! {
                            "aggregate_type": aggregate_type,
                            "aggregate_id": aggregate_id,
                            "sequence": sequence_num,
                        };
                        if let Ok(Some(reference_doc)) = collection.find_one(reference_query).await {
                            target_object_id = reference_doc.get_object_id("_id").ok();
                        }
                    }
                }
            }
            if target_object_id.is_none() {
                gap_detected = true;
            }
        }

        let mut events = Vec::new();

        if let Some(target_id) = target_object_id {
            let query = doc! { "_id": { "$gt": target_id } };
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
            let find_options = mongodb::options::FindOptions::builder()
                .sort(doc! { "_id": -1 })
                .build();

            let mut cursor = collection
                .find(doc! {})
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
            // Unbounded: query chronologically from the start
            let find_options = mongodb::options::FindOptions::builder().sort(doc! { "_id": 1 }).build();

            let mut cursor = collection
                .find(doc! {})
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
