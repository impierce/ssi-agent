use crate::connection::aggregate::Connection;
use crate::connection::views::all_connections::AllConnectionsView;
use crate::connection::views::ConnectionView;
use crate::document::aggregate::Status;
use crate::document::command::DocumentCommand;
use crate::document::error::DocumentError;
use crate::document::views::all_documents::AllDocumentsView;
use crate::profile::aggregate::{Profile, Source};
use crate::profile::command::ProfileCommand;
use crate::profile::views::ProfileView;
use crate::service::views::all_services::AllServicesView;
use crate::{
    document::{aggregate::Document, views::DocumentView},
    service::{aggregate::Service, views::ServiceView},
};
use agent_secret_manager::subject::{StorageKey, Subject};
use agent_shared::config::{config, config_mut, Display, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::{authorize_command, command_handler, CommandHandlerError};
use agent_shared::{
    application_state::{CommandHandler, CqrsComponentBuilder},
    handlers::public_query_handler,
};
use cqrs_es::persist::PersistenceError;
use identity_kit::did::DidUrl;
use itertools::iproduct;
use jsonwebtoken::Algorithm;
use shared_kernel::authorization::{AllowAllAuthorizationChecker, AuthorizationChecker, Caller, QueryOperation};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

/// The fixed identifier for the `Profile` aggregate, which is treated as a singleton.
///
/// This is for internal use only within the identity bounded context to ensure
/// all operations consistently target the one and only profile.
pub const PROFILE_ID: &str = "PROFILE-001";

impl QueryOperation for ConnectionView {
    const OPERATION_NAME: &'static str = "identity.connections.get";
}

impl QueryOperation for AllConnectionsView {
    const OPERATION_NAME: &'static str = "identity.connections.list";
}

impl QueryOperation for DocumentView {
    const OPERATION_NAME: &'static str = "identity.documents.get";
}

impl QueryOperation for AllDocumentsView {
    const OPERATION_NAME: &'static str = "identity.documents.list";
}

impl QueryOperation for ProfileView {
    const OPERATION_NAME: &'static str = "identity.profile.get";
}

impl QueryOperation for ServiceView {
    const OPERATION_NAME: &'static str = "identity.services.get";
}

impl QueryOperation for AllServicesView {
    const OPERATION_NAME: &'static str = "identity.services.list";
}

#[derive(Clone)]
pub struct IdentityState {
    pub services: Arc<crate::services::IdentityServices>,
    pub service_lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
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
    dyn DynViewRepository<ConnectionView, Connection>,
    dyn DynViewRepository<AllConnectionsView, Connection>,
    dyn DynViewRepository<DocumentView, Document>,
    dyn DynViewRepository<AllDocumentsView, Document>,
    dyn DynViewRepository<ProfileView, Profile>,
    dyn DynViewRepository<ServiceView, Service>,
    dyn DynViewRepository<AllServicesView, Service>,
>;

pub struct ViewRepositories<C1, C2, D1, D2, P, S1, S2>
where
    C1: DynViewRepository<ConnectionView, Connection> + ?Sized,
    C2: DynViewRepository<AllConnectionsView, Connection> + ?Sized,
    D1: DynViewRepository<DocumentView, Document> + ?Sized,
    D2: DynViewRepository<AllDocumentsView, Document> + ?Sized,
    P: DynViewRepository<ProfileView, Profile> + ?Sized,
    S1: DynViewRepository<ServiceView, Service> + ?Sized,
    S2: DynViewRepository<AllServicesView, Service> + ?Sized,
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

/// The unique identifier for the `LinkedDomains` service. Also becomes the service's fragment in
/// every published DID document.
pub const LINKED_DOMAINS_SERVICE_ID: &str = "linked-domains-service";

/// The unique identifier for the linked verifiable presentation service.
pub const LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState) -> anyhow::Result<()> {
    info!("Initializing the identity state ...");

    initialize_display(state).await?;
    let configuration = config().clone();
    initialize_documents(state, &configuration).await?;
    crate::service::lifecycle::maintain_services(state).await?;

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
    match public_query_handler(PROFILE_ID, &state.query.profile).await? {
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
        match public_query_handler(PROFILE_ID, &state.query.profile).await? {
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

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                if config_display.logo != persisted_logo {
                    let command = ProfileCommand::UpdateLogo {
                        logo: config_display.logo,
                        source: config_display_source.clone(),
                    };

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                if config_display.description != persisted_description {
                    let command = ProfileCommand::UpdateDescription {
                        description: config_display.description,
                        source: config_display_source.clone(),
                    };

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                if config_display.country != persisted_country {
                    let command = ProfileCommand::UpdateCountry {
                        country: config_display.country,
                        source: config_display_source.clone(),
                    };

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                let command = ProfileCommand::UpdateSource {
                    source: config_display_source,
                };

                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    PROFILE_ID,
                    &state.command.profile,
                    command,
                )
                .await?;
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

                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    PROFILE_ID,
                    &state.command.profile,
                    command,
                )
                .await?;
            }
        };
    } else {
        match public_query_handler(PROFILE_ID, &state.query.profile).await? {
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

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                if persisted_logo.is_some() {
                    let command = ProfileCommand::UpdateLogo {
                        logo: None,
                        source: config_display_source.clone(),
                    };

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
                }

                if persisted_country.is_some() {
                    let command = ProfileCommand::UpdateCountry {
                        country: None,
                        source: config_display_source.clone(),
                    };

                    command_handler(
                        state.authorization_checker.clone(),
                        Caller::Internal,
                        PROFILE_ID,
                        &state.command.profile,
                        command,
                    )
                    .await?;
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

                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    PROFILE_ID,
                    &state.command.profile,
                    command,
                )
                .await?;
            }
        };
    }

    query_profile(state).await?;

    Ok(())
}

/// Reuses the persisted deployment DID and refuses to start on public-origin drift unless the
/// configured one-shot overwrite authorization exactly names the persisted DID. Other DID
/// methods retain their existing configuration-driven initialization.
pub async fn initialize_documents(
    state: &IdentityState,
    configuration: &agent_shared::config::ApplicationConfiguration,
) -> anyhow::Result<()> {
    let did_methods_with_or_without_fixed_algorithm = get_did_methods_with_or_without_fixed_algorithm(configuration);

    let all_documents = query_all_documents(state, |_| true).await?;

    // Check identity drift before dispatching any document mutations.
    let persisted_documents: Vec<_> = all_documents
        .values()
        .filter(|document| document.did_method == Some(SupportedDidMethod::Web))
        .filter_map(|document| {
            document
                .document
                .as_ref()
                .map(|persisted| (document.document_id.clone(), persisted.id.clone()))
        })
        .collect();
    let mut did_web_overwritten = false;
    if !persisted_documents.is_empty() {
        let expected = crate::document::web::did_web(&configuration.public_url)?;
        let drifted_documents: Vec<_> = persisted_documents
            .iter()
            .filter(|(_, persisted)| persisted != &expected)
            .cloned()
            .collect();

        if !drifted_documents.is_empty() {
            let persisted_dids = drifted_documents
                .iter()
                .map(|(_, persisted)| persisted.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let configured_origin = configuration.public_url.origin().ascii_serialization();
            let previous_did = configuration.overwrite_previous_did_web.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Configured public origin '{configured_origin}' would change persisted deployment DID(s) \
                     [{persisted_dids}] to '{expected}'. Refusing to start without explicit overwrite authorization; \
                     set `overwrite_previous_did_web` to the persisted DID"
                )
            })?;
            let authorized_did = previous_did.parse::<identity_kit::did::Did>().map_err(|error| {
                anyhow::anyhow!("Invalid `overwrite_previous_did_web` value '{previous_did}': {error}")
            })?;
            if drifted_documents
                .iter()
                .any(|(_, persisted)| persisted != &authorized_did)
            {
                anyhow::bail!(
                    "Overwrite authorization '{authorized_did}' does not match persisted deployment DID(s) \
                     [{persisted_dids}]; refusing to change identity for configured origin '{configured_origin}'"
                );
            }

            warn!(
                persisted_did = %authorized_did,
                configured_did = %expected,
                configured_origin,
                "Explicitly overwriting the deployment did:web; existing credentials are not migrated"
            );
            for (document_id, persisted_did) in drifted_documents {
                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    &document_id,
                    &state.command.document,
                    DocumentCommand::OverwritePreviousDidWeb {
                        previous_did: persisted_did,
                        public_url: configuration.public_url.clone(),
                    },
                )
                .await?;
            }
            did_web_overwritten = true;
        } else {
            if let Some(previous_did) = &configuration.overwrite_previous_did_web {
                warn!(
                    overwrite_previous_did_web = previous_did,
                    current_did = %expected,
                    "DID overwrite authorization is no longer needed and should be removed from configuration"
                );
            }
        }
    }

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
            Some(document) if enabled && did_method == SupportedDidMethod::Web && document.document.is_some() => {
                if document.status == Status::Disabled {
                    Some((
                        document.document_id.clone(),
                        DocumentCommand::UpdateDocumentStatus {
                            status: Status::SignAndValidate,
                        },
                    ))
                } else {
                    None
                }
            }
            // Other DID methods retain their existing initialization behavior.
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
            let update_keys = !matches!(command, DocumentCommand::UpdateDocumentStatus { .. });
            command_handler(
                state.authorization_checker.clone(),
                Caller::Internal,
                &document_id,
                &state.command.document,
                command,
            )
            .await?;

            if enabled && update_keys {
                let command = DocumentCommand::UpdatePublicKeys {
                    public_key_jwks: vec![],
                };

                command_handler(
                    state.authorization_checker.clone(),
                    Caller::Internal,
                    &document_id,
                    &state.command.document,
                    command,
                )
                .await?;
            }
        }
    }

    cache_verification_method_ids(state).await?;

    if did_web_overwritten {
        crate::service::lifecycle::renew_existing_linked_domains_credentials(state).await?;
    }
    Ok(())
}

/// Caches the verification method each enabled document signs with, per algorithm. Signing caches are process-local,
/// even when the DID document is reused.
async fn cache_verification_method_ids(state: &IdentityState) -> anyhow::Result<()> {
    for document in query_all_documents(state, |(_, document)| document.status != Status::Disabled)
        .await?
        .values()
    {
        cache_signing_methods(&state.services.subject, document).await?;
    }

    Ok(())
}

/// Caches the verification method `document` signs with, per algorithm: the selected one, or else the one of its
/// configured issuer key. Added keys only sign once they are selected.
async fn cache_signing_methods(subject: &Subject, document: &Document) -> anyhow::Result<()> {
    let (Some(did_method), Some(did_document)) = (document.did_method, &document.document) else {
        return Ok(());
    };

    let configured_methods = did_document
        .methods(None)
        .filter(|method| !document.added_verification_methods.contains(&method.id))
        .filter_map(|method| {
            let alg = method.public_key_jwk().and_then(|jwk| jwk.alg())?;
            Some(alg.parse().map(|algorithm| (algorithm, method.id.clone())))
        })
        .collect::<Result<Vec<(Algorithm, DidUrl)>, _>>()?;
    for (algorithm, verification_method_id) in configured_methods {
        subject
            .insert_verification_method_id(StorageKey::new(did_method, algorithm), verification_method_id)
            .await?;
    }

    for (algorithm, verification_method_id) in &document.signing_methods {
        if !did_document
            .methods(None)
            .any(|method| method.id == *verification_method_id)
        {
            warn!(%verification_method_id, "The selected signing key is no longer in the DID document");
            continue;
        }
        if document.added_verification_methods.contains(verification_method_id)
            && !subject.has_method_key(verification_method_id).await
        {
            warn!(%verification_method_id, "The selected signing key is not available");
            continue;
        }
        subject
            .insert_verification_method_id(StorageKey::new(did_method, *algorithm), verification_method_id.clone())
            .await?;
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum SelectSigningKeyError {
    #[error("No document has the verification method `{0}`")]
    NotFound(DidUrl),
    #[error("The key of verification method `{0}` is not available to sign with")]
    KeyUnavailable(DidUrl),
    #[error(transparent)]
    Command(#[from] CommandHandlerError<DocumentError>),
    #[error(transparent)]
    Infrastructure(#[from] anyhow::Error),
}

/// Signs with the verification method `verification_method_id` from now on, for the algorithm of its key, in the
/// document of its DID.
///
/// # Errors
///
/// Fails when the caller isn't authorized, no document has the verification method, or the verification method's key
/// was added but isn't available to sign with.
pub async fn select_signing_key(
    state: &IdentityState,
    caller: Caller,
    verification_method_id: DidUrl,
) -> Result<(), SelectSigningKeyError> {
    let Some((document_id, document)) = query_all_documents(state, |(_, document)| {
        document
            .document
            .as_ref()
            .is_some_and(|did_document| did_document.id == *verification_method_id.did())
    })
    .await?
    .into_iter()
    .next() else {
        return Err(SelectSigningKeyError::NotFound(verification_method_id));
    };
    let command = DocumentCommand::SelectSigningKey {
        verification_method_id: verification_method_id.clone(),
    };
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &document_id,
        &command,
    )
    .await
    .map_err(CommandHandlerError::Authorization)?;

    if document.added_verification_methods.contains(&verification_method_id)
        && !state.services.subject.has_method_key(&verification_method_id).await
    {
        return Err(SelectSigningKeyError::KeyUnavailable(verification_method_id));
    }

    command_handler(
        state.authorization_checker.clone(),
        caller,
        &document_id,
        &state.command.document,
        command,
    )
    .await?;

    Ok(cache_verification_method_ids(state).await?)
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
fn get_did_methods_with_or_without_fixed_algorithm(
    configuration: &agent_shared::config::ApplicationConfiguration,
) -> Vec<((SupportedDidMethod, ToggleOptions), Option<Algorithm>)> {
    // Retrieve all the configured DID methods.
    let did_methods = configuration.did_methods.clone();

    // Retrieve all enabled signing algorithms, wrapping each in `Some`.
    let enabled_algorithms = configuration
        .signing_algorithms_supported
        .iter()
        .filter(|(_, options)| options.enabled)
        .map(|(algorithm, _)| Some(*algorithm));

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
            state.authorization_checker.clone(),
            Caller::Internal,
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
    match public_query_handler("all_documents", &state.query.all_documents).await? {
        Some(AllDocumentsView { documents }) => Ok(documents.into_iter().filter(query).collect()),
        None => Ok(Default::default()),
    }
}

/// Constructs the CQRS components and initializes the state for the Identity bounded context.
///
/// Registers command handlers and view repositories for connection, document, profile,
/// and service aggregates using the provided [`CqrsComponentBuilder`].
pub async fn identity_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    services: Arc<crate::services::IdentityServices>,
    event_bus: &EventBusHandle,
) -> IdentityState {
    let (connection_command_handler, connection, all_connections) = builder
        .commands_and_queries::<ConnectionView, Connection, AllConnectionsView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;
    let (document_command_handler, document, all_documents) = builder
        .commands_and_queries::<Document, Document, AllDocumentsView>(services.clone(), vec![event_bus.query()])
        .await;
    let (profile_command_handler, profile, _all_profiles) = builder
        .commands_and_queries::<Profile, Profile, Profile>(services.clone(), vec![event_bus.query()])
        .await;
    let (service_command_handler, service, all_services) = builder
        .commands_and_queries::<Service, Service, AllServicesView>(services.clone(), vec![event_bus.query()])
        .await;

    IdentityState {
        services,
        service_lifecycle_lock: Default::default(),
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: CommandHandlers {
            connection: connection_command_handler,
            document: document_command_handler,
            profile: profile_command_handler,
            service: service_command_handler,
        },
        query: ViewRepositories {
            connection,
            all_connections,
            document,
            all_documents,
            service,
            all_services,
            profile,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::aggregate::test_utils::{
        document_with_multiple_verification_methods, kms_public_key, kms_verification_method,
    };
    use agent_secret_manager::subject::MethodKey;
    use agent_store::in_memory::InMemory;
    use identity_kit::did::VerificationRelationship;
    use identity_kit::jose::JwsAlgorithm;
    use identity_kit::keys::{InMemoryKeyStore, KeySpec, KeyStore, KeyType};

    const CONFIGURED_EDDSA_METHOD: &str = "did:web:my-domain.example.org#bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0";
    const KMS_METHOD: &str = "did:web:my-domain.example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k";

    /// A key to sign for the KMS verification method with. Which key it is doesn't matter here.
    async fn method_key() -> MethodKey {
        let key_store = Arc::new(InMemoryKeyStore::new());
        let key_id = key_store
            .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA))
            .await
            .unwrap();

        MethodKey { key_store, key_id }
    }

    /// A `did:web` document with the configured issuer keys and an added KMS key, signing with `signing_methods`.
    fn did_web_document(signing_methods: impl IntoIterator<Item = (Algorithm, &'static str)>) -> Document {
        let mut did_document = document_with_multiple_verification_methods::default();
        did_document
            .insert_verification_method(kms_verification_method(), VerificationRelationship::ALL.iter().copied())
            .unwrap();

        Document {
            document_id: "did-web".to_string(),
            document: Some(did_document),
            did_method: Some(SupportedDidMethod::Web),
            status: Status::SignAndValidate,
            added_verification_methods: vec![kms_verification_method().id],
            signing_methods: signing_methods
                .into_iter()
                .map(|(algorithm, id)| (algorithm, id.parse().unwrap()))
                .collect(),
            ..Default::default()
        }
    }

    async fn eddsa_signing_method(subject: &Subject) -> String {
        subject
            .get_verification_method_id(StorageKey::new(SupportedDidMethod::Web, Algorithm::EdDSA))
            .await
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn an_added_key_does_not_sign_until_it_is_selected() {
        let subject = Subject::test_subject().await;
        subject
            .insert_method_key(KMS_METHOD.parse().unwrap(), method_key().await)
            .await;

        cache_signing_methods(&subject, &did_web_document([])).await.unwrap();

        assert_eq!(eddsa_signing_method(&subject).await, CONFIGURED_EDDSA_METHOD);
    }

    #[tokio::test]
    async fn a_selected_key_signs() {
        let subject = Subject::test_subject().await;
        subject
            .insert_method_key(KMS_METHOD.parse().unwrap(), method_key().await)
            .await;

        cache_signing_methods(&subject, &did_web_document([(Algorithm::EdDSA, KMS_METHOD)]))
            .await
            .unwrap();

        assert_eq!(eddsa_signing_method(&subject).await, KMS_METHOD);
    }

    #[tokio::test]
    async fn a_selected_key_that_is_not_available_does_not_sign() {
        let subject = Subject::test_subject().await;

        cache_signing_methods(&subject, &did_web_document([(Algorithm::EdDSA, KMS_METHOD)]))
            .await
            .unwrap();

        assert_eq!(eddsa_signing_method(&subject).await, CONFIGURED_EDDSA_METHOD);
    }

    #[tokio::test]
    async fn a_selected_key_that_left_the_document_does_not_sign() {
        let subject = Subject::test_subject().await;

        cache_signing_methods(
            &subject,
            &did_web_document([(Algorithm::EdDSA, "did:web:my-domain.example.org#replaced")]),
        )
        .await
        .unwrap();

        assert_eq!(eddsa_signing_method(&subject).await, CONFIGURED_EDDSA_METHOD);
    }

    async fn handle(state: &IdentityState, command: DocumentCommand) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            "did-web",
            &state.command.document,
            command,
        )
        .await
        .unwrap();
    }

    /// An identity state with a `did:web` document that has an added KMS key.
    async fn state_with_added_key() -> IdentityState {
        let state = identity_state(
            &InMemory,
            crate::services::IdentityServices::default(),
            &EventBusHandle::default(),
        )
        .await;
        handle(
            &state,
            DocumentCommand::CreateDocument {
                document_id: "did-web".to_string(),
                did_method: SupportedDidMethod::Web,
                with_fixed_algorithm: None,
            },
        )
        .await;
        handle(
            &state,
            DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            },
        )
        .await;
        handle(
            &state,
            DocumentCommand::AddPublicKey {
                public_key_jwk: Box::new(kms_public_key()),
            },
        )
        .await;

        state
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn selecting_a_key_signs_with_it_right_away() {
        let state = state_with_added_key().await;
        state
            .services
            .subject
            .insert_method_key(KMS_METHOD.parse().unwrap(), method_key().await)
            .await;

        select_signing_key(&state, Caller::Internal, KMS_METHOD.parse().unwrap())
            .await
            .unwrap();

        assert_eq!(eddsa_signing_method(&state.services.subject).await, KMS_METHOD);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn selecting_a_key_that_is_not_available_fails() {
        let state = state_with_added_key().await;

        let result = select_signing_key(&state, Caller::Internal, KMS_METHOD.parse().unwrap()).await;

        assert!(matches!(result, Err(SelectSigningKeyError::KeyUnavailable(_))));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn selecting_a_key_of_an_unknown_did_fails() {
        let state = state_with_added_key().await;

        let result = select_signing_key(&state, Caller::Internal, "did:web:other.example#key".parse().unwrap()).await;

        assert!(matches!(result, Err(SelectSigningKeyError::NotFound(_))));
    }
}
