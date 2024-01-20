use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::server_config::aggregate::ServerConfig;

pub struct SimpleLoggingQuery {}

#[async_trait]
impl Query<ServerConfig> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<ServerConfig>]) {}
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerConfigView {
    aggregate_id: Option<String>,
}

impl View<ServerConfig> for ServerConfigView {
    fn update(&mut self, event: &EventEnvelope<ServerConfig>) {
        println!("TODO: should be updating the view ...");
    }
}
