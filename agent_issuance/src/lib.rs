use async_trait::async_trait;
use cqrs_es::{Aggregate, EventEnvelope, Query};
use tracing::info;

pub mod credential;
pub mod handlers;
pub mod model;
pub mod offer;
// pub mod queries;
pub mod server_config;
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
