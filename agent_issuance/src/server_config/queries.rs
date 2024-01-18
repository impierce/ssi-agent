use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};

use crate::server_config::aggregate::ServerConfig;

pub struct SimpleLoggingQuery {}

#[async_trait]
impl Query<ServerConfig> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<ServerConfig>]) {}
}
