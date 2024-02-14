use async_trait::async_trait;
use cqrs_es::persist::{GenericQuery, ViewRepository};
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

#[async_trait]
pub trait CQRS<A>
where
    A: Aggregate,
{
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync;
}

#[derive(Clone)]
pub struct ApplicationState {
    pub server_config_handler: AggregateHandler<ServerConfig>,
    pub credential_handler: AggregateHandler<Credential>,
    pub offer_handler: AggregateHandler<Offer>,
    pub query: Queries<
        dyn ViewRepository<ServerConfigView, ServerConfig>,
        dyn ViewRepository<CredentialView, Credential>,
        dyn ViewRepository<OfferView, Offer>,
        dyn ViewRepository<PreAuthorizedCodeView, Offer>,
        dyn ViewRepository<AccessTokenView, Offer>,
    >,
}

pub type AggregateHandler<A> = Arc<dyn CQRS<A> + Send + Sync>;

pub struct Queries<SC, C, O, O1, O2>
where
    SC: ViewRepository<ServerConfigView, ServerConfig> + ?Sized,
    C: ViewRepository<CredentialView, Credential> + ?Sized,
    O: ViewRepository<OfferView, Offer> + ?Sized,
    O1: ViewRepository<PreAuthorizedCodeView, Offer> + ?Sized,
    O2: ViewRepository<AccessTokenView, Offer> + ?Sized,
{
    pub server_config: Arc<SC>,
    pub credential: Arc<C>,
    pub offer: Arc<O>,
    pub pre_authorized_code: Arc<O1>,
    pub access_token: Arc<O2>,
}

impl Clone
    for Queries<
        dyn ViewRepository<ServerConfigView, ServerConfig>,
        dyn ViewRepository<CredentialView, Credential>,
        dyn ViewRepository<OfferView, Offer>,
        dyn ViewRepository<PreAuthorizedCodeView, Offer>,
        dyn ViewRepository<AccessTokenView, Offer>,
    >
{
    fn clone(&self) -> Self {
        Queries {
            server_config: self.server_config.clone(),
            credential: self.credential.clone(),
            offer: self.offer.clone(),
            pre_authorized_code: self.pre_authorized_code.clone(),
            access_token: self.access_token.clone(),
        }
    }
}

pub fn generic_query<R, A, V>(view_repository: Arc<R>) -> GenericQuery<R, V, A>
where
    R: ViewRepository<V, A>,
    A: Aggregate,
    V: View<A>,
{
    let mut generic_query = GenericQuery::new(view_repository);
    generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

    generic_query
}

/// Initialize the application state by executing the startup commands.
pub async fn initialize(state: ApplicationState, startup_commands: Vec<ServerConfigCommand>) {
    info!("Initializing ...");

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler("SERVCONFIG-0001", &state.server_config_handler, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
