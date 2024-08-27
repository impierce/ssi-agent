use agent_shared::application_state::CommandHandler;
use agent_shared::handlers::command_handler;
use cqrs_es::persist::ViewRepository;
use std::sync::Arc;
use tracing::{info, warn};

use crate::credential::aggregate::Credential;
use crate::credential::queries::CredentialView;
use crate::offer::aggregate::Offer;
use crate::offer::queries::access_token::AccessTokenView;
use crate::offer::queries::pre_authorized_code::PreAuthorizedCodeView;
use crate::offer::queries::OfferView;
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::queries::ServerConfigView;

#[derive(Clone)]
pub struct IssuanceState {
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub server_config: CommandHandler<ServerConfig>,
    pub credential: CommandHandler<Credential>,
    pub offer: CommandHandler<Offer>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ServerConfigView, ServerConfig>,
    dyn ViewRepository<CredentialView, Credential>,
    dyn ViewRepository<OfferView, Offer>,
    dyn ViewRepository<PreAuthorizedCodeView, Offer>,
    dyn ViewRepository<AccessTokenView, Offer>,
>;

pub struct ViewRepositories<SC, C, O, O1, O2>
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

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            server_config: self.server_config.clone(),
            credential: self.credential.clone(),
            offer: self.offer.clone(),
            pre_authorized_code: self.pre_authorized_code.clone(),
            access_token: self.access_token.clone(),
        }
    }
}

/// The unique identifier for the server configuration.
pub const SERVER_CONFIG_ID: &str = "SERVER-CONFIG-001";

/// Initialize the application state by executing the startup commands.
pub async fn initialize(state: &IssuanceState, startup_commands: Vec<ServerConfigCommand>) {
    info!("Initializing ...");

    for command in startup_commands {
        let command_string = format!("{:?}", command).split(' ').next().unwrap().to_string();
        match command_handler(SERVER_CONFIG_ID, &state.command.server_config, command).await {
            Ok(_) => info!("Startup task completed: `{}`", command_string),
            Err(err) => warn!("Startup task failed: {:#?}", err),
        }
    }
}
