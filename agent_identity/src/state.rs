use crate::connection::aggregate::Connection;
use crate::connection::views::all_connections::AllConnectionsView;
use crate::connection::views::ConnectionView;
use crate::document::aggregate::Status;
use crate::document::command::DocumentCommand;
use crate::document::views::all_documents::AllDocumentsView;
use crate::service::views::all_services::AllServicesView;
use crate::{
    document::{aggregate::Document, views::DocumentView},
    service::{aggregate::Service, command::ServiceCommand, views::ServiceView},
};
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::command_handler;
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::ViewRepository;
use iota_sdk::client::api::GetAddressesOptions;
use iota_sdk::client::secret::SecretManager;
use iota_sdk::client::Client;
use iota_sdk::crypto::keys::bip39;
use iota_sdk::types::block::address::Bech32Address;
use iota_sdk::types::block::address::Hrp;
use itertools::iproduct;
use jsonwebtoken::Algorithm;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct IdentityState {
    pub command: CommandHandlers,
    pub query: Queries,
}

/// The command handlers are used to execute commands on the aggregates.
#[derive(Clone)]
pub struct CommandHandlers {
    pub connection: CommandHandler<Connection>,
    pub document: CommandHandler<Document>,
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
    dyn ViewRepository<ServiceView, Service>,
    dyn ViewRepository<AllServicesView, Service>,
>;

pub struct ViewRepositories<C1, C2, D1, D2, S1, S2>
where
    C1: ViewRepository<ConnectionView, Connection> + ?Sized,
    C2: ViewRepository<AllConnectionsView, Connection> + ?Sized,
    D1: ViewRepository<DocumentView, Document> + ?Sized,
    D2: ViewRepository<AllDocumentsView, Document> + ?Sized,
    S1: ViewRepository<ServiceView, Service> + ?Sized,
    S2: ViewRepository<AllServicesView, Service> + ?Sized,
{
    pub connection: Arc<C1>,
    pub all_connections: Arc<C2>,
    pub document: Arc<D1>,
    pub all_documents: Arc<D2>,
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
            service: self.service.clone(),
            all_services: self.all_services.clone(),
        }
    }
}

/// Initializes the [`SecretManager`] with a new mnemonic, if necessary,
/// and generates an address from the given [`SecretManager`].
pub async fn get_wallet_address(client: &Client, secret_manager: &SecretManager) -> anyhow::Result<Bech32Address> {
    let random: [u8; 32] = rand::random();
    let mnemonic = bip39::wordlist::encode(random.as_ref(), &bip39::wordlist::ENGLISH)
        .map_err(|err| anyhow::anyhow!(format!("{err:?}")))?;

    if let SecretManager::Stronghold(ref stronghold) = secret_manager {
        match stronghold.store_mnemonic(mnemonic).await {
            Ok(()) => (),
            Err(iota_sdk::client::stronghold::Error::MnemonicAlreadyStored) => (),
            Err(err) => anyhow::bail!(err),
        }
    } else {
        anyhow::bail!("expected a `StrongholdSecretManager`");
    }

    let bech32_hrp: Hrp = client.get_bech32_hrp().await?;
    let address: Bech32Address = secret_manager
        .generate_ed25519_addresses(
            GetAddressesOptions::default()
                .with_range(0..1)
                .with_bech32_hrp(bech32_hrp),
        )
        .await?[0];

    Ok(address)
}

/// The unique identifier for the linked domain service.
pub const DOMAIN_LINKAGE_SERVICE_ID: &str = "linked-domain-service";

/// The unique identifier for the linked verifiable presentation service.
pub const LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState) -> anyhow::Result<()> {
    info!("Initializing the identity state ...");

    initialize_documents(state).await?;
    initialize_domain_linkage(state).await?;
    initialize_linked_verifiable_presentations(state).await?;
    publish_decentrally_hosted_documents(state).await?;

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
                    document_id: document_id.clone(),
                    status: Status::Disabled,
                },
            )),
            // If the DID method is enabled, then create the Document regardless of whether it alraedy exists or not.
            document if enabled => {
                let document_id = document
                    // Extract the `document_id` from the Documument if it exists.
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
            command_handler(&document_id, &state.command.document, command).await?;

            if enabled {
                let command = DocumentCommand::UpdatePublicKeys {
                    document_id: document_id.clone(),
                    public_key_jwks: vec![],
                };

                command_handler(&document_id, &state.command.document, command).await?;
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
    })
    .await?;

    // Check whether Domain Linkage are enabled and whether there are any enabled update supporting Documents.
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

        command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command).await?;

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
                        service: service.clone(),
                    };

                    command_handler(document_id, &state.command.document, command).await?;
                }
            }
            _ => anyhow::bail!("Failed to retrieve Linked Domains service"),
        };
    } else {
        // If Domain Linkage is disabled and/or there are no enabled update supporting Documents, then disable the Domain Linkage Service.
        let command = ServiceCommand::DeleteDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
        };

        command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command).await?;

        info!("Disabled Domain Linkage service");
    }

    Ok(())
}

/// Initializes the Linked Verifiable Presentations service for DID Web Document.
pub async fn initialize_linked_verifiable_presentations(state: &IdentityState) -> anyhow::Result<()> {
    // TODO: We currently only support Linked Verifiable Presentations for DID Web. In the future we should also support
    // it for other update supporting DID methods.

    // Get the DID Web document.
    let did_web_document = query_all_documents(state, |(_, document)| {
        document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web)
    })
    .await?;

    if let Some(Service {
        service: Some(service), ..
    }) = query_handler(LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID, &state.query.service).await?
    {
        info!("Found Linked Verifiable Presentations service: {service}");

        // Add the Linked Verifiable Presentations service to the DID Web Document.
        for document_id in did_web_document.keys() {
            let command = DocumentCommand::AddService {
                service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                service: service.clone(),
            };

            command_handler(document_id, &state.command.document, command).await?;
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
        let command = DocumentCommand::PublishDocument {
            document_id: document_id.clone(),
        };

        command_handler(document_id, &state.command.document, command).await?;
    }

    Ok(())
}

/// Asynchronously retrieves all documents and filters them using the provided predicate.
///
/// This function uses the query handler to fetch all documents from the underlying data source,
/// then applies the specified predicate to each `(String, Document)` pair. Only those documents
/// for which the predicate returns `true` are included in the returned `HashMap`.
async fn query_all_documents(
    state: &IdentityState,
    query: impl Fn(&(String, Document)) -> bool,
) -> anyhow::Result<HashMap<String, Document>> {
    match query_handler("all_documents", &state.query.all_documents).await? {
        Some(AllDocumentsView { documents }) => Ok(documents.into_iter().filter(query).collect()),
        None => Ok(Default::default()),
    }
}
