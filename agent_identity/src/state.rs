use crate::connection::aggregate::Connection;
use crate::connection::views::all_connections::AllConnectionsView;
use crate::connection::views::ConnectionView;
use crate::document::aggregate::Status;
use crate::document::command::DocumentCommand;
use crate::document::views::all_documents::AllDocumentsView;
use crate::profile::aggregate::{Profile, Source};
use crate::profile::command::ProfileCommand;
use crate::profile::views::ProfileView;
use crate::service::views::all_services::AllServicesView;
use crate::{
    document::{aggregate::Document, views::DocumentView},
    service::{aggregate::Service, command::ServiceCommand, views::ServiceView},
};
use agent_shared::config::{
    config, config_mut, get_all_enabled_signing_algorithms_supported, Display, SupportedDidMethod, ToggleOptions,
};
use agent_shared::handlers::{command_handler, AuthorizationContext};
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::{PersistenceError, ViewRepository};
use itertools::iproduct;
use jsonwebtoken::Algorithm;
use shared_kernel::authorization::AuthorizationChecker;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// The fixed identifier for the `Profile` aggregate, which is treated as a singleton.
///
/// This is for internal use only within the identity bounded context to ensure
/// all operations consistently target the one and only profile.
pub const PROFILE_ID: &str = "PROFILE-001";

#[derive(Clone)]
pub struct IdentityState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
}

impl AuthorizationContext for IdentityState {
    fn authorization_checker(&self) -> &Arc<dyn AuthorizationChecker> {
        &self.authorization_checker
    }
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub connection: CommandHandler<Connection>,
    pub document: CommandHandler<Document>,
    pub profile: CommandHandler<Profile>,
    pub service: CommandHandler<Service>,
}

/// This type is used to define the queries that are used to query the view repositories. We make use of `dyn` here, so
/// that any type of repository that implements the `ViewRepository` trait can be used, but the corresponding `View` and
/// `Aggregate` types must be the same.
type Queries = ViewRepositories<
    dyn ViewRepository<ConnectionView, Connection>,
    dyn ViewRepository<AllConnectionsView, Connection>,
    dyn ViewRepository<DocumentView, Document>,
    dyn ViewRepository<AllDocumentsView, Document>,
    dyn ViewRepository<ProfileView, Profile>,
    dyn ViewRepository<ServiceView, Service>,
    dyn ViewRepository<AllServicesView, Service>,
>;

pub struct ViewRepositories<C1, C2, D1, D2, P, S1, S2>
where
    C1: ViewRepository<ConnectionView, Connection> + ?Sized,
    C2: ViewRepository<AllConnectionsView, Connection> + ?Sized,
    D1: ViewRepository<DocumentView, Document> + ?Sized,
    D2: ViewRepository<AllDocumentsView, Document> + ?Sized,
    P: ViewRepository<ProfileView, Profile> + ?Sized,
    S1: ViewRepository<ServiceView, Service> + ?Sized,
    S2: ViewRepository<AllServicesView, Service> + ?Sized,
{
    pub connection: Arc<C1>,
    pub all_connections: Arc<C2>,
    pub document: Arc<D1>,
    pub all_documents: Arc<D2>,
    pub profile: Arc<P>,
    pub service: Arc<S1>,
    pub all_services: Arc<S2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            connection: self.connection.clone(),
            all_connections: self.all_connections.clone(),
            document: self.document.clone(),
            all_documents: self.all_documents.clone(),
            profile: self.profile.clone(),
            service: self.service.clone(),
            all_services: self.all_services.clone(),
        }
    }
}

/// The unique identifier for the linked domain service.
pub const DOMAIN_LINKAGE_SERVICE_ID: &str = "linked-domain-service";

/// The unique identifier for the linked verifiable presentation service.
pub const LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState) -> anyhow::Result<()> {
    info!("Initializing the identity state ...");

    initialize_display(state).await?;
    initialize_documents(state).await?;
    initialize_domain_linkage(state).await?;
    initialize_linked_verifiable_presentations(state).await?;
    publish_decentrally_hosted_documents(state).await?;

    Ok(())
}

// TODO: This function is a temporary workaround and violates DDD principles.
// It directly mutates the global configuration state, which is an impure side effect.
//
// The correct long-term solution is to establish the Identity Bounded Context as the single
// source of truth for display data (name, logo). Other contexts, like Issuance and
// Verification, should subscribe to events published by the Identity Bounded Context to receive these
// updates, rather than reading from a shared, mutable global state.
/// Queries the profile and updates the application state with the profile information.
pub async fn query_profile(state: &IdentityState) -> Result<(), PersistenceError> {
    match query_handler(PROFILE_ID, &state.query.profile).await? {
        Some(Profile {
            display_name,
            logo,
            description,
            ..
        }) => {
            let hostname = config().application_url.host_str().unwrap_or_default().to_string();
            let display = &mut config_mut().display;
            if let Some(display) = display.first_mut() {
                display.name = display_name.unwrap_or_default();
                display.description = description;
                display.logo = logo;
            } else {
                display.push(Display {
                    name: display_name.unwrap_or(hostname),
                    description,
                    logo,
                    ..Default::default()
                });
            }

            Ok(())
        }
        None => {
            warn!("No profile found");

            Ok(())
        }
    }
}

// TODO: This function violates the aggregate's consistency boundary.
// The complex business logic for deciding whether to update the profile based on its
// source (Provisioned vs. Default vs. Runtime) should reside inside the `Profile` aggregate's `handle`
// method, not here in the application's initialization layer.
//
// A better approach would be:
// 1. This function should only read the config and dispatch a simple, declarative command,
//    e.g., `ProfileCommand::SynchronizeFromConfig { display_name: ..., logo: ... }`.
// 2. The `Profile` aggregate would then handle this command, containing all the logic
//    to protect its invariants (e.g., "if my source is `Runtime`, I must reject this command")
async fn initialize_display(state: &IdentityState) -> anyhow::Result<()> {
    // TODO: allow for multiple displays in the future with different locales.
    let first = config().display.first().cloned();

    let config_display_source = if config().is_display_provisioned() {
        Source::Provisioned
    } else {
        Source::Default
    };

    if let Some(config_display) = first {
        match query_handler(PROFILE_ID, &state.query.profile).await? {
            // If the profile exists, we check if it needs to be updated based on the config.
            // We only update the Profile if the config source is:
            // - Provisioned: If the Profile is Provisioned, we update the persisted Profile.
            // - Default: If the persisted Profile is Provisioned, we update the persisted Profile.
            Some(Profile {
                display_name: persisted_display_name,
                description: persisted_description,
                logo: persisted_logo,
                country: persisted_country,
                source: persisted_source,
                ..
            }) if (config_display_source == Source::Provisioned
                || (config_display_source == Source::Default && persisted_source == Source::Provisioned)) =>
            {
                if Some(&config_display.name) != persisted_display_name.as_ref() {
                    let command = ProfileCommand::UpdateDisplayName {
                        display_name: config_display.name,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                if config_display.logo != persisted_logo {
                    let command = ProfileCommand::UpdateLogo {
                        logo: config_display.logo,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                if config_display.description != persisted_description {
                    let command = ProfileCommand::UpdateDescription {
                        description: config_display.description,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                if config_display.country != persisted_country {
                    let command = ProfileCommand::UpdateCountry {
                        country: config_display.country,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                let command = ProfileCommand::UpdateSource {
                    source: config_display_source,
                };

                command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
            }
            Some(_profile) => {
                info!("Display is already configured, no action needed.");
            }
            // If the profile does not exist, we create it with the config display information.
            None => {
                info!("No display configured, creating a new one.");

                let command = ProfileCommand::CreateProfile {
                    profile_id: PROFILE_ID.to_string(),
                    display_name: Some(config_display.name.clone()),
                    description: config_display.description.clone(),
                    logo: config_display.logo.clone(),
                    source: config_display_source,
                    country: config_display.country.clone(),
                };

                command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
            }
        };
    } else {
        match query_handler(PROFILE_ID, &state.query.profile).await? {
            Some(Profile {
                display_name: persisted_display_name,
                logo: persisted_logo,
                country: persisted_country,
                source: Source::Provisioned,
                ..
            }) => {
                if persisted_display_name.is_some() {
                    let command = ProfileCommand::UpdateDisplayName {
                        display_name: "".to_string(),
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                if persisted_logo.is_some() {
                    let command = ProfileCommand::UpdateLogo {
                        logo: None,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }

                if persisted_country.is_some() {
                    let command = ProfileCommand::UpdateCountry {
                        country: None,
                        source: config_display_source.clone(),
                    };

                    command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
                }
            }
            _ => {
                let command = ProfileCommand::CreateProfile {
                    profile_id: PROFILE_ID.to_string(),
                    display_name: None,
                    description: None,
                    logo: None,
                    country: None,
                    source: config_display_source,
                };

                command_handler(&state, PROFILE_ID, &state.command.profile, command).await?;
            }
        };
    }

    query_profile(state).await?;

    Ok(())
}

/// Initializes or updates documents based on the current DID methods configuration.
///
/// This asynchronous function synchronizes document state with the configured DID methods by:
///
/// 1. Retrieving all DID methods along with their fixed algorithm information via
///    `get_did_methods_with_or_without_fixed_algorithm()`.
/// 2. Querying all existing documents using `query_all_documents`, thereby obtaining a map
///    of document entries.
/// 3. Iterating over each DID method:
///    - If a document exists with the matching DID method and fixed algorithm flag and the DID method
///      is disabled (i.e. `ToggleOptions.enabled` is `false`), the document's status is updated to
///      `Disabled`.
///    - If the DID method is enabled, a document is created (or updated) regardless of whether it
///      already exists. If a document already exists, its `document_id` is reused; otherwise, a new
///      one is generated.
/// 4. For each generated document command, executing the command via `command_handler` and subsequently
///    updating the document's public keys.
async fn initialize_documents(state: &IdentityState) -> anyhow::Result<()> {
    let did_methods_with_or_without_fixed_algorithm = get_did_methods_with_or_without_fixed_algorithm();

    let all_documents = query_all_documents(state, |_| true).await?;

    for ((did_method, ToggleOptions { enabled, .. }), with_fixed_algorithm) in
        did_methods_with_or_without_fixed_algorithm
    {
        let document_id_and_command = match all_documents.values().find(|document| {
            document.did_method == Some(did_method) && document.with_fixed_algorithm == with_fixed_algorithm
        }) {
            // If the Document already exists, but the DID method is not enabled, then update the Document's status to `Disabled`.
            Some(Document {
                document_id,
                status: Status::SignAndValidate,
                ..
            }) if !enabled => Some((
                document_id.clone(),
                DocumentCommand::UpdateDocumentStatus {
                    status: Status::Disabled,
                },
            )),
            // If the DID method is enabled, then create the Document regardless of whether it already exists or not.
            document if enabled => {
                let document_id = document
                    // Extract the `document_id` from the Document if it exists.
                    .map(|document| document.document_id.clone())
                    // Otherwise, generate a new `document_id`.
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                Some((
                    document_id.clone(),
                    DocumentCommand::CreateDocument {
                        document_id: document_id.clone(),
                        did_method,
                        with_fixed_algorithm,
                    },
                ))
            }
            // In all other cases, the DID method is disabled and therefore no action is required.
            _disabled => None,
        };

        // If a Document command was generated, then execute the command and update the Document's Public Keys.
        if let Some((document_id, command)) = document_id_and_command {
            command_handler(&state, &document_id, &state.command.document, command).await?;

            if enabled {
                let command = DocumentCommand::UpdatePublicKeys {
                    public_key_jwks: vec![],
                };

                command_handler(&state, &document_id, &state.command.document, command).await?;
            }
        }
    }

    Ok(())
}

/// Constructs pairs of configured DID methods with their associated signing algorithms.
///
/// For DID methods that do not support updates, this function creates a Cartesian product between
/// each such method and every enabled signing algorithm (wrapping each algorithm in `Some`), thereby
/// allowing multiple algorithm options per method. In contrast, for DID methods that support updates,
/// the signing algorithm is not required, so they are paired with `None`.
///
/// # Returns
///
/// A `Vec` of tuples where:
/// - The first element is a tuple of a `SupportedDidMethod` and its `ToggleOptions`.
/// - The second element is an `Option<Algorithm>`, where:
///   - `Some(algorithm)` indicates a fixed signing algorithm for non-update supporting DID methods.
///   - `None` indicates that the DID method supports updates and does not require a fixed algorithm.
fn get_did_methods_with_or_without_fixed_algorithm() -> Vec<((SupportedDidMethod, ToggleOptions), Option<Algorithm>)> {
    // Retrieve all the configured DID methods.
    let did_methods = config().did_methods.clone();

    // Retrieve all enabled signing algorithms, wrapping each in `Some`.
    let enabled_algorithms = get_all_enabled_signing_algorithms_supported().into_iter().map(Some);

    // Partition DID methods into those that support updates and those that do not.
    let (update_supporting_did_methods, non_update_supporting_did_methods): (Vec<_>, Vec<_>) = did_methods
        .into_iter()
        .partition(|(did_method, _)| did_method.supports_update());

    // For non-update supporting DID methods, create a pair for each enabled algorithm.
    iproduct!(non_update_supporting_did_methods.into_iter(), enabled_algorithms)
        // For update supporting DID methods, pair each with None (indicating the DID method does not require a fixed algorithm).
        .chain(
            update_supporting_did_methods
                .into_iter()
                .map(|did_method| (did_method, None)),
        )
        .collect()
}

/// Initializes or disables the Domain Linkage Service based on the current configuration and document state.
///
/// This asynchronous function performs the following steps:
///
/// 1. Query Documents: It retrieves all documents that are not disabled and whose DID methods support updates.
/// 2. Conditional Service Creation:
///    - If domain linkage is enabled in the configuration and there exists at least one update-supporting document,
///      it creates the Domain Linkage Service.
///    - It then queries for the created service. If found, it adds the service to all update-supporting documents.
/// 3. Service Deletion:
///    - If domain linkage is disabled or no update-supporting documents exist, the function sends a command
///      to disable the Domain Linkage Service.
pub async fn initialize_domain_linkage(state: &IdentityState) -> anyhow::Result<()> {
    // Get all the Documents that are not disabled and support updates.
    let update_supporting_documents = query_all_documents(state, |(_, document)| {
        document.status != Status::Disabled
            && document
                .did_method
                .as_ref()
                .map(SupportedDidMethod::supports_update)
                .unwrap_or_default()
            && document
                .iota_metadata
                .as_ref()
                .map(|iota_metadata| iota_metadata.is_funded || config().iota_sponsoring_service_url.is_some())
                .unwrap_or(true)
    })
    .await?;

    // Check whether Domain Linkage is enabled and whether there are any enabled update-supporting Documents.
    if config().domain_linkage_enabled && !update_supporting_documents.is_empty() {
        info!(
            "Creating domain linkage service with documents: {:?}",
            update_supporting_documents
        );

        // Collect all Verification Methods from update-supporting documents.
        let verification_methods = update_supporting_documents
            .values()
            .filter_map(|document| document.document.as_ref())
            .flat_map(|core_document| core_document.methods(None).into_iter().cloned())
            .collect();

        // Create the Domain Linkage Service.
        let command = ServiceCommand::CreateDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
            verification_methods,
        };

        command_handler(&state, DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command).await?;

        info!("Created Linked Domain service");

        match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await {
            Ok(Some(Service {
                service: Some(service), ..
            })) => {
                info!("Found Linked Domains service: {service}");

                // Add the Domain Linkage service to all the enabled update supporting Documents.
                for document_id in update_supporting_documents.keys() {
                    let command = DocumentCommand::AddService {
                        service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                        service: Box::new(service.clone()),
                    };

                    command_handler(&state, document_id, &state.command.document, command).await?;
                }
            }
            _ => anyhow::bail!("Failed to retrieve Linked Domains service"),
        };
    } else {
        // If Domain Linkage is disabled and/or there are no enabled update supporting Documents, then disable the Domain Linkage Service.
        let command = ServiceCommand::DeleteDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
        };

        command_handler(&state, DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command).await?;

        info!("Disabled Domain Linkage service");
    }

    Ok(())
}

/// Initializes the Linked Verifiable Presentations service for DID Web Document.
pub async fn initialize_linked_verifiable_presentations(state: &IdentityState) -> anyhow::Result<()> {
    // Get all documents that can be updated.
    let documents = query_all_documents(state, |(_, document)| {
        document.status != Status::Disabled
            && document
                .did_method
                .as_ref()
                .map(SupportedDidMethod::supports_update)
                .unwrap_or(false)
    })
    .await?;

    if let Some(Service {
        service: Some(service), ..
    }) = query_handler(LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID, &state.query.service).await?
    {
        info!("Found Linked Verifiable Presentations service: {service}");

        // Add the Linked Verifiable Presentations service to the DID Web Document.
        for document_id in documents.keys() {
            let command = DocumentCommand::AddService {
                service_id: LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.to_string(),
                service: Box::new(service.clone()),
            };

            command_handler(&state, document_id, &state.command.document, command).await?;
        }
    }

    Ok(())
}

/// Publishes all decentrally hosted documents.
///
/// This asynchronous function performs the following steps:
///
/// 1. Query Documents: It retrieves all documents whose associated DID methods indicate they are
///    hosted decentrally by filtering with the `SupportedDidMethod::hosted_decentrally` predicate.
/// 2. Publish Documents: For each decentrally hosted document found, it sends a `PublishDocument`
///    command via the command handler to publish the document.
pub async fn publish_decentrally_hosted_documents(state: &IdentityState) -> anyhow::Result<()> {
    // Get all the decentrally hosted Documents.
    let decentrally_hosted_documents = query_all_documents(state, |(_, document)| {
        document
            .did_method
            .as_ref()
            .map(SupportedDidMethod::hosted_decentrally)
            .unwrap_or_default()
    })
    .await?;

    // Publish each decentrally hosted Documents.
    for document_id in decentrally_hosted_documents.keys() {
        // Publish the Document. Note that we ignore any errors here to allow for the system to continue initializing.
        let _ = command_handler(
            &state,
            document_id,
            &state.command.document,
            DocumentCommand::PublishDocument,
        )
        .await;
    }

    Ok(())
}

// TODO: Make this function generic and move it to a shared module.
/// Asynchronously retrieves all documents and filters them using the provided predicate.
///
/// This function uses the query handler to fetch all documents from the underlying data source,
/// then applies the specified predicate to each `(String, Document)` pair. Only those documents
/// for which the predicate returns `true` are included in the returned `HashMap`.
pub async fn query_all_documents(
    state: &IdentityState,
    query: impl Fn(&(String, Document)) -> bool,
) -> anyhow::Result<HashMap<String, Document>> {
    match query_handler("all_documents", &state.query.all_documents).await? {
        Some(AllDocumentsView { documents }) => Ok(documents.into_iter().filter(query).collect()),
        None => Ok(Default::default()),
    }
}
