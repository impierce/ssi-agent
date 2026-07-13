use agent_secret_manager::subject::Subject;
use agent_shared::application_state::CommandHandler;
use agent_shared::config::{config, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported};
use agent_shared::handlers::{command_handler, public_query_handler};
use agent_shared::UrlAppendHelpers;
use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use shared_kernel::authorization::{AuthorizationChecker, Caller};
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;
use tracing::{debug, info};

use crate::credential::aggregate::Credential;
use crate::credential::views::all_credentials::AllCredentialsView;
use crate::credential::views::CredentialView;
use crate::nonce::aggregate::Nonce;
use crate::nonce::views::NonceView;
use crate::offer::aggregate::Offer;
use crate::offer::views::all_offers::AllOffersView;
use crate::offer::views::OfferView;
use crate::public_offer::aggregate::PublicOffer;
use crate::public_offer::views::{AllPublicOffersView, PublicOfferView};
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::views::ServerConfigView;
use crate::status_list::aggregate::StatusListAggregate;
use crate::status_list::views::all_status_lists::AllStatusListsView;
use crate::status_list::views::StatusListView;

#[derive(Clone)]
pub struct IssuanceState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
    pub subject: Arc<Subject>,
}

impl std::fmt::Debug for IssuanceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuanceState")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
        // We intentionally do not include the command handlers and queries in the debug output, as they don't contain useful information.
    }
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub server_config: CommandHandler<ServerConfig>,
    pub credential: CommandHandler<Credential>,
    pub offer: CommandHandler<Offer>,
    pub nonce: CommandHandler<Nonce>,
    pub status_list: CommandHandler<StatusListAggregate>,
    pub public_offer: CommandHandler<PublicOffer>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn DynViewRepository<ServerConfigView, ServerConfig>,
    dyn DynViewRepository<CredentialView, Credential>,
    dyn DynViewRepository<AllCredentialsView, Credential>,
    dyn DynViewRepository<OfferView, Offer>,
    dyn DynViewRepository<AllOffersView, Offer>,
    dyn DynViewRepository<NonceView, Nonce>,
    dyn DynViewRepository<StatusListView, StatusListAggregate>,
    dyn DynViewRepository<AllStatusListsView, StatusListAggregate>,
    dyn DynViewRepository<PublicOfferView, PublicOffer>,
    dyn DynViewRepository<AllPublicOffersView, PublicOffer>,
>;

pub struct ViewRepositories<SC, C, C1, O, O1, N, SL, SL1, PO, PO1>
where
    SC: DynViewRepository<ServerConfigView, ServerConfig> + ?Sized,
    C: DynViewRepository<CredentialView, Credential> + ?Sized,
    C1: DynViewRepository<AllCredentialsView, Credential> + ?Sized,
    O: DynViewRepository<OfferView, Offer> + ?Sized,
    O1: DynViewRepository<AllOffersView, Offer> + ?Sized,
    N: DynViewRepository<NonceView, Nonce> + ?Sized,
    SL: DynViewRepository<StatusListView, StatusListAggregate> + ?Sized,
    SL1: DynViewRepository<AllStatusListsView, StatusListAggregate> + ?Sized,
    PO: DynViewRepository<PublicOfferView, PublicOffer> + ?Sized,
    PO1: DynViewRepository<AllPublicOffersView, PublicOffer> + ?Sized,
{
    pub server_config: Arc<SC>,
    pub credential: Arc<C>,
    pub all_credentials: Arc<C1>,
    pub offer: Arc<O>,
    pub all_offers: Arc<O1>,
    pub nonce: Arc<N>,
    pub status_list: Arc<SL>,
    pub all_status_lists: Arc<SL1>,
    pub public_offer: Arc<PO>,
    pub all_public_offers: Arc<PO1>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            server_config: self.server_config.clone(),
            credential: self.credential.clone(),
            all_credentials: self.all_credentials.clone(),
            offer: self.offer.clone(),
            all_offers: self.all_offers.clone(),
            nonce: self.nonce.clone(),
            status_list: self.status_list.clone(),
            all_status_lists: self.all_status_lists.clone(),
            public_offer: self.public_offer.clone(),
            all_public_offers: self.all_public_offers.clone(),
        }
    }
}

/// The unique identifier for the server configuration.
pub const SERVER_CONFIG_ID: &str = "SERVER-CONFIG-001";

/// Initialize the application state by executing the startup commands.
pub async fn initialize(state: &IssuanceState) -> anyhow::Result<()> {
    info!("Initializing the issuance state ...");

    load_server_metadata(state).await?;
    update_cryptographic_binding_methods(state).await?;
    update_signing_algorithms(state).await?;

    Ok(())
}

pub async fn load_server_metadata(state: &IssuanceState) -> anyhow::Result<()> {
    let public_url = config().public_url.clone();

    let display = config().display.first().map(|display| {
        let display = serde_json::json!(display);
        vec![display]
    });

    let mut cryptographic_binding_methods_supported: Vec<_> = get_all_enabled_did_methods()
        .into_iter()
        .map(|did_method| did_method.to_string())
        .collect();

    cryptographic_binding_methods_supported.sort();

    let signing_algorithms_supported = get_all_enabled_signing_algorithms_supported();

    match public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
        Some(server_config_view) => {
            info!("Update Issuer URL in server metadata");

            let command = ServerConfigCommand::UpdateIssuerUrl {
                url: public_url.clone(),
            };
            command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                SERVER_CONFIG_ID,
                &state.command.server_config,
                command,
            )
            .await?;

            if display != server_config_view.credential_issuer_metadata.display {
                debug!("The server metadata display does not match the configured display.");

                let command = ServerConfigCommand::UpdateIssuerDisplay { display };
                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    SERVER_CONFIG_ID,
                    &state.command.server_config,
                    command,
                )
                .await?;
            }
        }
        None => {
            info!("Initializing server metadata ...");

            // If `enable_interactive_authorization_flow` is enabled, then the `require_pushed_authorization_requests`
            // field will be set to `None`, and the `interactive_authorization_endpoint` and
            // `require_interactive_authorization_request` fields will be set to the corresponding values. If
            // `enable_interactive_authorization_flow` is disabled, then the `require_pushed_authorization_requests`
            // field will be set to `Some(true)`, and the `interactive_authorization_endpoint` and
            // `require_interactive_authorization_request` fields will be set to `None`.
            // Keep in mind: the pre-authorized code flow is still supported, independent of what is enabled/required here.
            let (
                require_pushed_authorization_requests,
                interactive_authorization_endpoint,
                require_interactive_authorization_request,
            ) = if config().enable_interactive_authorization_flow {
                info!("Interactive authorization flow is enabled. Initializing interactive authorization endpoints in server metadata.");

                (None, Some(public_url.append_path_segment("auth/par")), Some(true))
            } else {
                info!("Interactive authorization flow is disabled. Interactive authorization endpoints will not be included in the server metadata.");

                (Some(true), None, None)
            };

            let command = ServerConfigCommand::InitializeServerMetadata {
                // TODO: Move this to `agent_authorization`.
                authorization_server_metadata: Box::new(AuthorizationServerMetadata {
                    issuer: public_url.clone(),
                    authorization_endpoint: Some(public_url.append_path_segment("auth/authorize")),
                    token_endpoint: Some(public_url.append_path_segment("auth/token")),
                    pushed_authorization_request_endpoint: Some(public_url.append_path_segment("auth/par")),
                    require_pushed_authorization_requests,
                    interactive_authorization_endpoint,
                    require_interactive_authorization_request,
                    ..Default::default()
                }),
                credential_issuer_metadata: Box::new(CredentialIssuerMetadata {
                    credential_issuer: public_url.clone(),
                    credential_endpoint: public_url.append_path_segment("openid4vci/credential"),
                    nonce_endpoint: Some(public_url.append_path_segment("openid4vci/nonce")),
                    display,
                    ..Default::default()
                }),
                cryptographic_binding_methods_supported,
                signing_algorithms_supported,
            };

            command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                SERVER_CONFIG_ID,
                &state.command.server_config,
                command,
            )
            .await?;
        }
    }

    Ok(())
}

pub async fn update_cryptographic_binding_methods(state: &IssuanceState) -> anyhow::Result<()> {
    if let Some(server_config_view) = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
        let mut cryptographic_binding_methods_supported: Vec<_> = get_all_enabled_did_methods()
            .into_iter()
            .map(|did_method| did_method.to_string())
            .collect();

        cryptographic_binding_methods_supported.sort();

        if server_config_view.cryptographic_binding_methods_supported != cryptographic_binding_methods_supported {
            let command = ServerConfigCommand::UpdateCryptographicBindingMethods {
                cryptographic_binding_methods_supported,
            };

            command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                SERVER_CONFIG_ID,
                &state.command.server_config,
                command,
            )
            .await?;
        } else {
            debug!("Cryptographic binding methods are already up to date.");
        }
    }

    Ok(())
}

pub async fn update_signing_algorithms(state: &IssuanceState) -> anyhow::Result<()> {
    if let Some(server_config_view) = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await? {
        let signing_algorithms_supported = get_all_enabled_signing_algorithms_supported();

        if server_config_view.signing_algorithms_supported != signing_algorithms_supported {
            let command = ServerConfigCommand::UpdateSigningAlgorithms {
                signing_algorithms_supported,
            };

            command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                SERVER_CONFIG_ID,
                &state.command.server_config,
                command,
            )
            .await?;
        } else {
            debug!("Signing algorithms are already up to date.");
        }
    }

    Ok(())
}
