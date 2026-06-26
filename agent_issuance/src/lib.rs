// This line is added to allow for large Json strings to be serialized by the `json!` macro.
#![recursion_limit = "256"]

use async_trait::async_trait;
use cqrs_es::{Aggregate, EventEnvelope, Query};
use tracing::info;

// Aggregates
pub mod credential;
pub mod nonce;
pub mod offer;
pub mod public_offer;
pub mod server_config;
pub mod status_list;
pub mod utils;

pub mod application;
pub mod services;
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
