use async_trait::async_trait;
use cqrs_es::persist::PersistenceError;
use cqrs_es::{Aggregate, AggregateError, Query, View};
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
use crate::offer::queries::OfferView;
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
pub trait CQRS<D: Domain> {
    async fn new(
        queries: Vec<Box<dyn Query<D::Aggregate>>>,
        services: <<D as Domain>::Aggregate as Aggregate>::Services,
    ) -> AggregateHandler<D>
    where
        Self: Sized;

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: <D::Aggregate as Aggregate>::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<<D::Aggregate as Aggregate>::Error>>
    where
        <D::Aggregate as Aggregate>::Command: Send + Sync;

    //     async fn execute_with_metadata_credential(
    //         &self,
    //         aggregate_id: &str,
    //         command: CredentialCommand,
    //         metadata: HashMap<String, String>,
    //     ) -> Result<(), AggregateError<CredentialError>>;

    //     async fn execute_with_metadata_offer(
    //         &self,
    //         aggregate_id: &str,
    //         command: OfferCommand,
    //         metadata: HashMap<String, String>,
    //     ) -> Result<(), AggregateError<OfferError>>;

    async fn load(&self, view_id: &str) -> Result<Option<D::View>, PersistenceError>;
}

pub trait Domain {
    type Aggregate: Aggregate;
    type View: View<Self::Aggregate>;
}

#[derive(Clone)]
pub struct ApplicationState {
    pub server_config: AggregateHandler<ServerConfig>,
    pub credential: AggregateHandler<Credential>,
    pub offer: AggregateHandler<Offer>,
}

pub type AggregateHandler<D> = Arc<dyn CQRS<D> + Send + Sync>;
// pub type ApplicationState = Arc<dyn Send + Sync>;

/// Initialize the application state by executing the startup commands.
pub async fn initialize<D: Domain>(
    state: AggregateHandler<D>,
    startup_commands: Vec<<D::Aggregate as Aggregate>::Command>,
) where
    <D::Aggregate as Aggregate>::Command: Send + Sync + std::fmt::Debug,
{
    info!("Initializing ...");

    // let _ = command_handler_credential("CRED_001".to_string(), &state, load_credential_format_template()).await;

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler("CONFIG_001".to_string(), &state, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
