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
    async fn open(&self, from: SubscribePosition) -> Result<BusEventStream, EventBusError> {
        if matches!(from, SubscribePosition::From(_)) {
            return Err(EventBusError::UnsupportedPosition);
        }

        let database = self
            .client
            .default_database()
            .ok_or_else(|| EventBusError::Source("No default database configured on MongoDB client".to_string()))?;

        let _ = database.create_collection("events").await;
        let collection = database.collection::<bson::Document>("events");

        let change_stream = collection
            .watch()
            .await
            .map_err(|e| EventBusError::Source(e.to_string()))?;

        let stream = change_stream.filter_map(|change_result| match change_result {
            Ok(change) => {
                let doc = change.full_document?;

                let aggregate_type = doc.get_str("aggregate_type").ok()?;
                let aggregate_id = doc.get_str("aggregate_id").ok()?;
                let sequence = doc.get_i64("sequence").ok()? as usize;
                let event_type = doc.get_str("event_type").ok()?;

                let payload_bson = doc.get("payload")?;
                let payload: serde_json::Value = bson::from_bson(payload_bson.clone()).ok()?;

                let metadata: Option<serde_json::Value> =
                    doc.get("metadata").and_then(|b| bson::from_bson(b.clone()).ok());

                let occurred_at = metadata
                    .as_ref()
                    .and_then(|m| m.get("occurred_at"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc));

                let cloud_event =
                    build_cloud_event(aggregate_type, aggregate_id, sequence, event_type, payload, occurred_at);

                Some(Ok(cloud_event))
            }
            Err(e) => Some(Err(EventBusError::Source(e.to_string()))),
        });

        Ok(Box::pin(stream))
    }
}
