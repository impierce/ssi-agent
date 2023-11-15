use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};

use crate::model::aggregate::Credential;

pub struct TempQuery {}

// Our simplest query, this is great for debugging but absolutely useless in production.
// This query just pretty prints the events as they are processed.
#[async_trait]
impl Query<Credential> for TempQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Credential>]) {
        for event in events {
            let payload = serde_json::to_string_pretty(&event.payload).unwrap();
            println!("{}-{}\n{}", aggregate_id, event.sequence, payload);
        }
    }
}
