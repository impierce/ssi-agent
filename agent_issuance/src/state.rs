use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, View};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use crate::credential::aggregate::Credential;
use crate::credential::queries::CredentialView;
use crate::handlers::command_handler;
use crate::offer::aggregate::Offer;
use crate::offer::queries::{AccessTokenView, OfferView, PreAuthorizedCodeView};
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::queries::ServerConfigView;

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
