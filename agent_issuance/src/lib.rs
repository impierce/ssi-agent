use async_trait::async_trait;
use cqrs_es::{Aggregate, EventEnvelope, Query};
use tracing::info;

// Aggregates
pub mod credential;
pub mod offer;
pub mod server_config;

pub mod services;
pub mod startup_commands;
pub mod state;

pub struct SimpleLoggingQuery {}

#[async_trait]
impl<A: Aggregate> Query<A> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<A>]) {
        for event in events {
            let payload = serde_json::to_string_pretty(&event.payload).unwrap();
            info!("{}-{} - {}", aggregate_id, event.sequence, payload);
        }
    }
}
