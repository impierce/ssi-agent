use async_trait::async_trait;
use cqrs_es::persist::{PersistenceError, ViewRepository};
use cqrs_es::{Aggregate, AggregateError, Query, View};
use oid4vci::credential_offer::PreAuthorizedCode;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::credential::aggregate::Credential;
use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError;
use crate::credential::queries::CredentialView;
use crate::credential::services::CredentialServices;
use crate::handlers::command_handler;
use crate::offer::aggregate::Offer;
// use crate::handlers::{command_handler_credential, command_handler_server_config};
use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError;
use crate::offer::queries::{AccessTokenView, OfferView, PreAuthorizedCodeView};
use crate::offer::services::OfferServices;
// use crate::handlers::command_handler;
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::error::ServerConfigError;
use crate::server_config::queries::ServerConfigView;
use crate::server_config::services::ServerConfigServices;
use crate::startup_commands::load_credential_format_template;

#[allow(clippy::new_ret_no_self)]
#[async_trait]
pub trait CQRS<A, V>
where
    A: Aggregate,
    V: View<A>,
{
    async fn new() -> AggregateHandler<A, V>
    where
        Self: Sized;
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync;

    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError>;

    async fn load_pre_authorized_code(
        &self,
        _view_id: &str,
    ) -> Result<Option<PreAuthorizedCodeView>, PersistenceError> {
        Ok(None)
    }

    async fn load_access_token(&self, _view_id: &str) -> Result<Option<AccessTokenView>, PersistenceError> {
        Ok(None)
    }
}

#[derive(Clone)]
pub struct ApplicationState {
    pub offer: AggregateHandler<Offer, OfferView>,
    pub credential: AggregateHandler<Credential, CredentialView>,
    pub server_config: AggregateHandler<ServerConfig, ServerConfigView>,
}

pub type AggregateHandler<A, V> = Arc<dyn CQRS<A, V> + Send + Sync>;
// pub type ApplicationState = Arc<dyn Send + Sync>;

/// Initialize the application state by executing the startup commands.
pub async fn initialize(state: ApplicationState, startup_commands: Vec<ServerConfigCommand>) {
    info!("Initializing ...");

    // let _ = command_handler_credential("CRED_001".to_string(), &state, load_credential_format_template()).await;

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler("SERVCONFIG-0001".to_string(), &state.server_config, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
