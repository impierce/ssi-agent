use async_trait::async_trait;
use mongo_es::Client;
use mongodb::bson;
use shared_kernel::event_bus::{build_cloud_event, BusEventStream, EventBusError, EventSource, SubscribePosition};
use tokio_stream::StreamExt;

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
            .map_err(|e| EventBusError::Source(e.to_string()))?;

        let stream = change_stream.filter_map(|change_result| match change_result {
            Ok(change) => {
                let Some(doc) = change.full_document else {
                    tracing::warn!("Change stream event missing full_document");
                    return None;
                };

                let aggregate_type = match doc.get_str("aggregate_type") {
                    Ok(at) => at,
                    Err(err) => {
                        tracing::warn!("Failed to parse aggregate_type from document: {}", err);
                        return None;
                    }
                };

                let aggregate_id = match doc.get_str("aggregate_id") {
                    Ok(aid) => aid,
                    Err(err) => {
                        tracing::warn!("Failed to parse aggregate_id from document: {}", err);
                        return None;
                    }
                };

                let sequence = match doc.get_i64("sequence") {
                    Ok(seq) => seq as usize,
                    Err(err) => {
                        tracing::warn!("Failed to parse sequence from document: {}", err);
                        return None;
                    }
                };

                let event_type = match doc.get_str("event_type") {
                    Ok(et) => et,
                    Err(err) => {
                        tracing::warn!("Failed to parse event_type from document: {}", err);
                        return None;
                    }
                };

                let payload_bson = match doc.get("payload") {
                    Some(p) => p,
                    None => {
                        tracing::warn!("Missing payload in change stream document");
                        return None;
                    }
                };

                let payload: serde_json::Value = match bson::from_bson(payload_bson.clone()) {
                    Ok(p) => p,
                    Err(err) => {
                        tracing::warn!("Failed to deserialize payload BSON into JSON: {}", err);
                        return None;
                    }
                };

                let metadata_doc = doc.get_document("metadata").ok();

                let occurred_at = metadata_doc.as_ref().and_then(|m| {
                    if let Ok(b_dt) = m.get_datetime("time_stamp").or_else(|_| m.get_datetime("occurred_at")) {
                        return chrono::DateTime::from_timestamp_millis(b_dt.timestamp_millis());
                    }
                    if let Ok(ts_str) = m.get_str("time_stamp").or_else(|_| m.get_str("occurred_at")) {
                        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                            return Some(dt.with_timezone(&chrono::Utc));
                        }
                    }
                    None
                });

                let cloud_event =
                    build_cloud_event(aggregate_type, aggregate_id, sequence, event_type, payload, occurred_at);

                Some(Ok(cloud_event))
            }
            Err(e) => Some(Err(EventBusError::Source(e.to_string()))),
        });

        Ok(Box::pin(stream))
    }
}
