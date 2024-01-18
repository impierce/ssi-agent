use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, AggregateError, Query, View};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError;
use crate::handlers::{command_handler_credential, command_handler_server_config};
use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError;
// use crate::handlers::command_handler;
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::services::ServerConfigServices;
use crate::startup_commands::load_credential_format_template;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait CQRS {
    async fn new(
        server_config_queries: Vec<Box<dyn Query<ServerConfig>>>,
        server_config_services: ServerConfigServices,
    ) -> ApplicationState
    where
        Self: Sized;

    async fn execute_with_metadata_server_config(
        &self,
        aggregate_id: &str,
        command: ServerConfigCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<ServerConfigError>>;

    async fn execute_with_metadata_credential(
        &self,
        aggregate_id: &str,
        command: CredentialCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<CredentialError>>;
    // async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;

    async fn execute_with_metadata_offer(
        &self,
        aggregate_id: &str,
        command: OfferCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<OfferError>>;

    async fn load(&self, view_id: &str) -> Result<Option<serde_json::Value>, PersistenceError>;
}

pub type ApplicationState = Arc<dyn CQRS + Send + Sync>;

/// Initialize the application state by executing the startup commands.
pub async fn initialize(state: ApplicationState, startup_commands: Vec<ServerConfigCommand>)
// where
//     <A as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    info!("Initializing ...");

    let _ = command_handler_credential("CRED_001".to_string(), &state, load_credential_format_template()).await;

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler_server_config("CONFIG_001".to_string(), &state, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
