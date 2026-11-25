use agent_secret_manager::subject::Subject;
use agent_shared::application_state::{CommandHandler, CqrsComponentBuilder, CqrsComponents};
use agent_shared::config::{config, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported};
use agent_shared::handlers::{command_handler, public_query_handler};
use agent_shared::UrlAppendHelpers;
use cqrs_es::Aggregate as _;
use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use shared_kernel::authorization::{AllowAllAuthorizationChecker, AuthorizationChecker, Caller, QueryOperation};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::credential::aggregate::Credential;
use crate::credential::views::all_credentials::AllCredentialsView;
use crate::credential::views::CredentialView;
use crate::nonce::aggregate::Nonce;
use crate::nonce::views::NonceView;
use crate::offer::aggregate::Offer;
use crate::offer::views::all_offers::AllOffersView;
use crate::offer::views::OfferView;
use crate::ports::IssuancePorts;
use crate::public_offer::aggregate::PublicOffer;
use crate::public_offer::views::{AllPublicOffersView, PublicOfferView};
use crate::refresh::aggregate::RefreshCapability;
use crate::refresh::views::{AllRefreshCapabilitiesView, RefreshCapabilityView};
use crate::reissuance::aggregate::Reissuance;
use crate::reissuance::views::all_reissuances::AllReissuancesView;
use crate::reissuance::views::ReissuanceView;
use crate::server_config::aggregate::{credential_signing_alg_values_supported, ServerConfig};
use crate::server_config::command::ServerConfigCommand;
use crate::server_config::views::ServerConfigView;
use crate::status_list::aggregate::StatusListAggregate;
use crate::status_list::views::all_status_lists::AllStatusListsView;
use crate::status_list::views::StatusListView;

impl QueryOperation for ServerConfigView {
    const OPERATION_NAME: &'static str = "issuance.configurations.get";
}

impl QueryOperation for CredentialView {
    const OPERATION_NAME: &'static str = "issuance.credentials.get";
}

impl QueryOperation for AllCredentialsView {
    const OPERATION_NAME: &'static str = "issuance.credentials.list";
}

impl QueryOperation for ReissuanceView {
    const OPERATION_NAME: &'static str = "issuance.credential_reissuances.get";
}

impl QueryOperation for AllReissuancesView {
    const OPERATION_NAME: &'static str = "issuance.credential_reissuances.list";
}

impl QueryOperation for OfferView {
    const OPERATION_NAME: &'static str = "issuance.offers.get";
}

impl QueryOperation for AllOffersView {
    const OPERATION_NAME: &'static str = "issuance.offers.list";
}

impl QueryOperation for NonceView {
    const OPERATION_NAME: &'static str = "issuance.nonces.get";
}

impl QueryOperation for StatusListView {
    const OPERATION_NAME: &'static str = "issuance.status_lists.get";
}

impl QueryOperation for AllStatusListsView {
    const OPERATION_NAME: &'static str = "issuance.status_lists.list";
}

impl QueryOperation for PublicOfferView {
    const OPERATION_NAME: &'static str = "issuance.public_offers.get";
}

impl QueryOperation for AllPublicOffersView {
    const OPERATION_NAME: &'static str = "issuance.public_offers.list";
}

#[derive(Clone)]
pub struct IssuanceState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    /// The decisions left to the application. [`issuance_state`] installs the defaults; the application replaces
    /// them once, where it is composed.
    pub ports: IssuancePorts,
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
    pub reissuance: CommandHandler<Reissuance>,
    pub offer: CommandHandler<Offer>,
    pub nonce: CommandHandler<Nonce>,
    pub status_list: CommandHandler<StatusListAggregate>,
    pub public_offer: CommandHandler<PublicOffer>,
    pub refresh_capability: CommandHandler<RefreshCapability>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn DynViewRepository<ServerConfigView, ServerConfig>,
    dyn DynViewRepository<CredentialView, Credential>,
    dyn DynViewRepository<AllCredentialsView, Credential>,
    dyn DynViewRepository<ReissuanceView, Reissuance>,
    dyn DynViewRepository<AllReissuancesView, Reissuance>,
    dyn DynViewRepository<OfferView, Offer>,
    dyn DynViewRepository<AllOffersView, Offer>,
    dyn DynViewRepository<NonceView, Nonce>,
    dyn DynViewRepository<StatusListView, StatusListAggregate>,
    dyn DynViewRepository<AllStatusListsView, StatusListAggregate>,
    dyn DynViewRepository<PublicOfferView, PublicOffer>,
    dyn DynViewRepository<AllPublicOffersView, PublicOffer>,
    dyn DynViewRepository<RefreshCapabilityView, RefreshCapability>,
    dyn DynViewRepository<AllRefreshCapabilitiesView, RefreshCapability>,
>;

/// The view repositories of the issuance aggregates, one per view: an aggregate's single view under its own name and
/// its listing under `all_*`. Generic over the repository types so that [`Queries`] can hold any
/// [`DynViewRepository`]; every view is read through it, never written.
pub struct ViewRepositories<SC, C, C1, R, R1, O, O1, N, SL, SL1, PO, PO1, RC, RC1>
where
    SC: DynViewRepository<ServerConfigView, ServerConfig> + ?Sized,
    C: DynViewRepository<CredentialView, Credential> + ?Sized,
    C1: DynViewRepository<AllCredentialsView, Credential> + ?Sized,
    R: DynViewRepository<ReissuanceView, Reissuance> + ?Sized,
    R1: DynViewRepository<AllReissuancesView, Reissuance> + ?Sized,
    O: DynViewRepository<OfferView, Offer> + ?Sized,
    O1: DynViewRepository<AllOffersView, Offer> + ?Sized,
    N: DynViewRepository<NonceView, Nonce> + ?Sized,
    SL: DynViewRepository<StatusListView, StatusListAggregate> + ?Sized,
    SL1: DynViewRepository<AllStatusListsView, StatusListAggregate> + ?Sized,
    PO: DynViewRepository<PublicOfferView, PublicOffer> + ?Sized,
    PO1: DynViewRepository<AllPublicOffersView, PublicOffer> + ?Sized,
    RC: DynViewRepository<RefreshCapabilityView, RefreshCapability> + ?Sized,
    RC1: DynViewRepository<AllRefreshCapabilitiesView, RefreshCapability> + ?Sized,
{
    pub server_config: Arc<SC>,
    pub credential: Arc<C>,
    pub all_credentials: Arc<C1>,
    pub reissuance: Arc<R>,
    pub all_reissuances: Arc<R1>,
    pub offer: Arc<O>,
    pub all_offers: Arc<O1>,
    pub nonce: Arc<N>,
    pub status_list: Arc<SL>,
    pub all_status_lists: Arc<SL1>,
    pub public_offer: Arc<PO>,
    pub all_public_offers: Arc<PO1>,
    /// Refresh capabilities by token hash. Read without authorization: the refresh token is the authorization.
    pub refresh_capability: Arc<RC>,
    pub all_refresh_capabilities: Arc<RC1>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            server_config: self.server_config.clone(),
            credential: self.credential.clone(),
            all_credentials: self.all_credentials.clone(),
            reissuance: self.reissuance.clone(),
            all_reissuances: self.all_reissuances.clone(),
            offer: self.offer.clone(),
            all_offers: self.all_offers.clone(),
            nonce: self.nonce.clone(),
            status_list: self.status_list.clone(),
            all_status_lists: self.all_status_lists.clone(),
            public_offer: self.public_offer.clone(),
            all_public_offers: self.all_public_offers.clone(),
            refresh_capability: self.refresh_capability.clone(),
            all_refresh_capabilities: self.all_refresh_capabilities.clone(),
        }
    }
}

/// The aggregates whose events are never published: refresh capabilities. Event sources that read the event store
/// directly, such as the MongoDB change stream, leave them out.
pub const UNPUBLISHED_AGGREGATE_TYPES: [&str; 1] = [RefreshCapability::TYPE];

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

/// Logs one warning that names every credential configuration advertising `refreshService`, if any. The wiring calls
/// it when it installed [`RefuseEveryRefresh`](crate::refresh::preparation::RefuseEveryRefresh): those credentials then
/// embed a refresh URL that answers 403. Run it once the configurations are loaded, after
/// [`resync_credential_configurations`](crate::application::credential_configuration_projection::resync_credential_configurations).
pub async fn warn_about_refreshable_configurations(state: &IssuanceState) {
    let server_config = match public_query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(server_config) => server_config.unwrap_or_default(),
        Err(error) => {
            warn!(%error, "Failed to check the credential configurations for refresh");
            return;
        }
    };
    let mut refreshable: Vec<_> = server_config
        .credential_configurations
        .iter()
        .filter(|(_, (_, configuration, _))| configuration.refresh_service.is_some())
        .map(|(credential_configuration_id, _)| credential_configuration_id.as_str())
        .collect();
    if refreshable.is_empty() {
        return;
    }
    refreshable.sort_unstable();
    warn!(
        configurations = refreshable.join(", "),
        "Credential configurations advertise `refreshService`, but no refresh preparation hook is set: refreshing \
         their credentials answers 403"
    );
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
            // Checked on the view first: a command loads the aggregate, which replays every server
            // config event.
            let (authorization_server_metadata, credential_issuer_metadata) =
                server_config_view.with_issuer_url(&public_url);
            if authorization_server_metadata != server_config_view.authorization_server_metadata
                || credential_issuer_metadata != server_config_view.credential_issuer_metadata
            {
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
            }

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
                    credential_issuer: public_url.clone().into(),
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

        let credential_signing_alg_values_supported = credential_signing_alg_values_supported();
        let advertises_other_credential_signing_algorithms = server_config_view.credential_configurations.values().any(
            |(_provisioned, credential_configuration, _authorization_grant)| {
                credential_configuration
                    .metadata
                    .credential_signing_alg_values_supported
                    != credential_signing_alg_values_supported
            },
        );

        if server_config_view.signing_algorithms_supported != signing_algorithms_supported
            || advertises_other_credential_signing_algorithms
        {
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

struct RefreshCapabilityComponents {
    command: CommandHandler<RefreshCapability>,
    view: Arc<dyn DynViewRepository<RefreshCapabilityView, RefreshCapability>>,
    all_views: Arc<dyn DynViewRepository<AllRefreshCapabilitiesView, RefreshCapability>>,
}

impl From<CqrsComponents<RefreshCapability, RefreshCapabilityView, AllRefreshCapabilitiesView>>
    for RefreshCapabilityComponents
{
    fn from(components: CqrsComponents<RefreshCapability, RefreshCapabilityView, AllRefreshCapabilitiesView>) -> Self {
        Self {
            command: components.0,
            view: components.1,
            all_views: components.2,
        }
    }
}

/// Constructs the CQRS components and initializes the state for the Issuance bounded context.
///
/// Registers command handlers and view repositories for credentials, credential offers,
/// public offers, server configurations, nonces, and status lists using the provided [`CqrsComponentBuilder`].
pub async fn issuance_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<crate::services::IssuanceServices>,
    event_bus: &EventBusHandle,
) -> IssuanceState {
    let (credential_command_handler, credential, all_credentials) = builder
        .commands_and_queries::<CredentialView, Credential, AllCredentialsView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;
    let (reissuance_command_handler, reissuance, all_reissuances) = builder
        .commands_and_queries::<ReissuanceView, Reissuance, AllReissuancesView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;
    let (offer_command_handler, offer, all_offers) = builder
        .commands_and_queries::<OfferView, Offer, AllOffersView>(services.clone(), vec![event_bus.query()])
        .await;
    let (public_offer_command_handler, public_offer, all_public_offers) = builder
        .commands_and_queries::<PublicOfferView, PublicOffer, AllPublicOffersView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;
    let (server_config_command_handler, server_config, _all_server_configs) = builder
        .commands_and_queries::<ServerConfigView, ServerConfig, ServerConfig>(services.clone(), vec![event_bus.query()])
        .await;
    let (nonce_command_handler, nonce, _) = builder
        .commands_and_queries::<NonceView, Nonce, NonceView>(services.clone(), vec![event_bus.query()])
        .await;
    // Refresh capabilities are never published, so they get no event bus query.
    let refresh_capabilities = RefreshCapabilityComponents::from(
        builder
            .commands_and_queries::<RefreshCapabilityView, RefreshCapability, AllRefreshCapabilitiesView>(
                services.clone(),
                vec![],
            )
            .await,
    );
    let (status_list_command_handler, status_list, all_status_lists) = builder
        .commands_and_queries::<StatusListView, StatusListAggregate, AllStatusListsView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;

    IssuanceState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        ports: IssuancePorts::default(),
        command: CommandHandlers {
            credential: credential_command_handler,
            reissuance: reissuance_command_handler,
            offer: offer_command_handler,
            public_offer: public_offer_command_handler,
            server_config: server_config_command_handler,
            nonce: nonce_command_handler,
            status_list: status_list_command_handler,
            refresh_capability: refresh_capabilities.command,
        },
        query: ViewRepositories {
            server_config,
            credential,
            all_credentials,
            reissuance,
            all_reissuances,
            offer,
            all_offers,
            public_offer,
            all_public_offers,
            nonce,
            status_list,
            all_status_lists,
            refresh_capability: refresh_capabilities.view,
            all_refresh_capabilities: refresh_capabilities.all_views,
        },
        subject: services.issuer.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        refresh::command::RefreshCapabilityCommand, reissuance::command::ReissuanceCommand, services::IssuanceServices,
    };
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use shared_kernel::event_bus::EventFilter;

    async fn state(event_bus: &EventBusHandle) -> IssuanceState {
        issuance_state(&InMemory, IssuanceServices::default().await, event_bus).await
    }

    #[tokio::test]
    async fn reissuance_events_reach_publishers_that_list_them() {
        let event_bus = EventBusHandle::default();
        let state = state(&event_bus).await;

        agent_shared::handlers::public_command_handler(
            "reissuance",
            &state.command.reissuance,
            ReissuanceCommand::CreateReissuance {
                reissuance_id: "reissuance".to_owned(),
                original_credential_id: "original".to_owned(),
                new_credential_id: "replacement".to_owned(),
                offer_id: "offer".to_owned(),
                credential_configuration_id: "configuration".to_owned(),
                reason: None,
                trigger_type: None,
                triggered_by: None,
            },
        )
        .await
        .unwrap();

        let listed = EventFilter {
            event_types: vec!["ReissuanceCreated".to_owned()],
            ..Default::default()
        };
        let published: Vec<_> = event_bus
            .history(&listed, 10)
            .into_iter()
            .map(|event| event.event_type)
            .collect();
        assert_eq!(published, vec!["com.impierce.unicore.reissuance-created"]);
    }

    #[tokio::test]
    async fn published_credential_events_keep_no_refresh_token() {
        use crate::{
            application::credential_creation::create_credential,
            credential::{
                aggregate::{test_utils::DC_SD_JWT_CREDENTIAL_CONFIGURATION, CredentialExpiry},
                command::CredentialCommand,
                entity::Data,
            },
            server_config::supported_configuration::SupportedCredentialConfiguration,
        };
        let event_bus = EventBusHandle::default();
        event_bus.set_disclosure(Arc::new(crate::refresh::disclosure::RefreshTokenDisclosure));
        let state = state(&event_bus).await;
        let credential_id = "11111111-1111-4111-8111-111111111111";
        create_credential(
            &state,
            Caller::Internal,
            credential_id,
            "configuration",
            CredentialCommand::CreateUnsignedCredential {
                credential_id: credential_id.to_owned(),
                data: Data {
                    raw: serde_json::json!({"first_name": "Ferris"}),
                },
                credential_configuration: Box::new(SupportedCredentialConfiguration {
                    metadata: DC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
                    refresh_service: Some(agent_shared::config::RefreshServiceConfiguration {
                        type_: "VerifiableCredentialRefreshService2021".to_owned(),
                    }),
                }),
                expires_at: CredentialExpiry::Never,
                refresh_token: None,
            },
        )
        .await
        .unwrap();

        agent_shared::handlers::public_command_handler(
            credential_id,
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: credential_id.to_owned(),
                subject_id: None,
                overwrite: false,
                proofs: None,
                status_list_id: "status-list".to_owned(),
                index: 0,
            },
        )
        .await
        .unwrap();

        // Newest first: `CredentialSigned`, then `UnsignedCredentialCreated`.
        let published: Vec<_> = event_bus
            .history(&EventFilter::default(), 10)
            .into_iter()
            .map(|event| event.data.unwrap())
            .collect();
        assert_eq!(
            (
                published[1]["data"]["raw"]["refreshService"].get("refreshToken"),
                &published[0]["signed_credential"]
            ),
            (None, &serde_json::Value::Null)
        );
    }

    #[tokio::test]
    async fn refresh_capability_events_are_never_published() {
        let event_bus = EventBusHandle::default();
        let state = state(&event_bus).await;

        agent_shared::handlers::public_command_handler(
            "capability",
            &state.command.refresh_capability,
            RefreshCapabilityCommand::CreateRefreshCapability {
                credential_id: "credential".to_owned(),
                credential_configuration_id: "configuration".to_owned(),
            },
        )
        .await
        .unwrap();

        assert!(event_bus.history(&EventFilter::default(), 10).is_empty());
    }
}
