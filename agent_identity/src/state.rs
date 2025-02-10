use crate::connection::aggregate::Connection;
use crate::connection::views::all_connections::AllConnectionsView;
use crate::connection::views::ConnectionView;
use crate::document::aggregate::Status;
use crate::document::command::DocumentCommand;
use crate::service::views::all_services::AllServicesView;
use crate::{
    document::{aggregate::Document, views::DocumentView},
    service::{aggregate::Service, command::ServiceCommand, views::ServiceView},
};
use agent_shared::config::{config, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::command_handler;
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::ViewRepository;
use futures::future::{join_all, try_join_all};
use iota_sdk::client::api::GetAddressesOptions;
use iota_sdk::client::secret::SecretManager;
use iota_sdk::client::Client;
use iota_sdk::crypto::keys::bip39;
use iota_sdk::types::block::address::Bech32Address;
use iota_sdk::types::block::address::Hrp;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

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
    dyn ViewRepository<ServiceView, Service>,
    dyn ViewRepository<AllServicesView, Service>,
>;

pub struct ViewRepositories<C1, C2, D, S1, S2>
where
    C1: ViewRepository<ConnectionView, Connection> + ?Sized,
    C2: ViewRepository<AllConnectionsView, Connection> + ?Sized,
    D: ViewRepository<DocumentView, Document> + ?Sized,
    S1: ViewRepository<ServiceView, Service> + ?Sized,
    S2: ViewRepository<AllServicesView, Service> + ?Sized,
{
    pub connection: Arc<C1>,
    pub all_connections: Arc<C2>,
    pub document: Arc<D>,
    pub service: Arc<S1>,
    pub all_services: Arc<S2>,
}

impl Clone for Queries {
    fn clone(&self) -> Self {
        ViewRepositories {
            connection: self.connection.clone(),
            all_connections: self.all_connections.clone(),
            document: self.document.clone(),
            service: self.service.clone(),
            all_services: self.all_services.clone(),
        }
    }
}

/// Initializes the [`SecretManager`] with a new mnemonic, if necessary,
/// and generates an address from the given [`SecretManager`].
pub async fn get_address(client: &Client, secret_manager: &SecretManager) -> anyhow::Result<Bech32Address> {
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
pub const VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState) {
    info!("Initializing ...");

    let did_methods = config().did_methods.clone().into_iter().collect::<Vec<_>>();

    info!("DID Methods: {:?}", did_methods);

    let documents: Vec<_> = join_all(
        // Loop through all DID methods.
        did_methods
            .iter()
            .map(|(did_method, ToggleOptions { enabled, .. })| async {
                // Clone the variables into the async closure.
                let did_method = did_method.clone();
                let document_id = did_method.to_string();

                // Check whether the DID methods document already exists.
                let command = match query_handler(&document_id, &state.query.document).await {
                    // If the DID document exists, then the status needs to be updated.
                    Ok(Some(_document_exists)) => {
                        if *enabled {
                            DocumentCommand::SetStatus {
                                did_method: did_method.clone(),
                                status: Status::SignAndValidate,
                            }
                        } else {
                            DocumentCommand::SetStatus {
                                did_method: did_method.clone(),
                                status: Status::Disabled,
                            }
                        }
                    }
                    // If the DID document does not exist yet, then it needs to be created.
                    _document_does_not_exist => {
                        if *enabled {
                            DocumentCommand::CreateDocument {
                                did_method: did_method.clone(),
                            }
                        } else {
                            return Err(format!("DID Document for `{did_method}` does not exist"));
                        }
                    }
                };

                if command_handler(&document_id, &state.command.document, command)
                    .await
                    .is_err()
                {
                    warn!("5: Failed to Set status `{did_method}`");
                }

                let command = DocumentCommand::SetPublicKeyJwks {
                    did_method: did_method.clone(),
                    public_key_jwks: vec![],
                };

                if command_handler(&document_id, &state.command.document, command)
                    .await
                    .is_err()
                {
                    warn!("5: Failed to Set status `{did_method}`");
                }

                match query_handler(&document_id, &state.query.document).await {
                    Ok(Some(document)) => Ok(document),
                    _ => Err(format!("DID Document for `{}` does not exist", did_method)),
                }
            })
            .collect::<Vec<_>>(),
    )
    .await
    .into_iter()
    .filter_map(|result| result.ok())
    .collect();

    info!("Documents: {:?}", documents);

    let enabled_updateable_documents = documents
        .clone()
        .into_iter()
        .filter_map(|document| {
            if document.status != Status::Disabled {
                document.document
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if config().domain_linkage_enabled && !enabled_updateable_documents.is_empty() {
        info!(
            "Creating domain linkage service with documents: {:?}",
            enabled_updateable_documents
        );

        let command = ServiceCommand::CreateDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
            documents: enabled_updateable_documents,
        };

        if command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command)
            .await
            .is_err()
        {
            warn!("Failed to create domain linkage service");
        }

        info!("Created domain linkage service");

        match query_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.query.service).await {
            Ok(Some(Service {
                service: Some(service), ..
            })) => {
                info!("Found linked domains service: {service}");

                try_join_all(
                    // Loop through all DID methods.
                    documents
                        .iter()
                        .map(|document| async {
                            // Clone the variables into the async closure.
                            let document_id = document.document_id.clone();
                            let did_method = SupportedDidMethod::from_str(&document_id).unwrap();
                            let service = service.clone();

                            let command = match document.status {
                                Status::Disabled => DocumentCommand::RemoveService {
                                    service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                },
                                Status::SignAndValidate => DocumentCommand::AddService {
                                    service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                    service,
                                },
                            };

                            if command_handler(&document_id, &state.command.document, command)
                                .await
                                .is_err()
                            {
                                warn!("7: Failed to add service to document");
                            }

                            info!("8: Added service to document for `{}`", did_method);

                            Ok::<(), ()>(())
                        })
                        .collect::<Vec<_>>(),
                )
                .await
                .unwrap();
            }
            _ => {
                warn!("Failed to retrieve linked domains service");
                return;
            }
        };
    } else {
        let command = ServiceCommand::DeleteDomainLinkageService {
            service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
        };

        if command_handler(DOMAIN_LINKAGE_SERVICE_ID, &state.command.service, command)
            .await
            .is_err()
        {
            warn!("Failed to deleted domain linkage service");
        }

        info!("Domain linkage service is disabled");

        try_join_all(
            // Loop through all DID methods.
            documents
                .iter()
                .map(|document| async {
                    // Clone the variables into the async closure.
                    let document_id = document.document_id.clone();

                    let command = DocumentCommand::RemoveService {
                        service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                    };

                    command_handler(&document_id, &state.command.document, command).await
                })
                .collect::<Vec<_>>(),
        )
        .await
        .expect("FIX THISS");
    }

    try_join_all(
        // Loop through all DID methods.
        did_methods
            .iter()
            .map(|(did_method, _)| async {
                // Clone the variables into the async closure.
                let did_method = did_method.clone();
                let document_id = did_method.to_string();

                if did_method.is_decentrally_hosted() {
                    let command = DocumentCommand::PublishDocument {
                        did_method: did_method.clone(),
                    };

                    info!("Publishing document for `{}`", did_method);

                    if command_handler(&document_id, &state.command.document, command)
                        .await
                        .is_err()
                    {
                        warn!("9: Failed to publish DID Document for `{did_method}`");
                    }
                }

                info!("10: Published document for `{}`", did_method);

                Ok::<(), ()>(())
            })
            .collect::<Vec<_>>(),
    )
    .await
    .unwrap();
}
