use agent_secret_manager::{stronghold_storage, ED25519_KEY_ID, STRONGHOLD_PATH};
use agent_shared::config::{config, SupportedDidMethod, ToggleOptions};
use agent_shared::handlers::command_handler;
use agent_shared::{application_state::CommandHandler, handlers::query_handler};
use cqrs_es::persist::ViewRepository;
use did_manager::StrongholdExtStorage;
use futures::future::{join_all, try_join_all};
use identity_iota::core::Duration;
use identity_iota::credential::Jws;
use identity_iota::storage::{KeyId, KeyType};
use identity_stronghold::{StrongholdKeyType, StrongholdStorage};
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use iota_sdk::types::block::output::{AliasOutputBuilder, RentStructure};
use iota_stronghold::{SnapshotPath, Stronghold};
use jsonwebtoken::Algorithm;
use oid4vc_core::Subject;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

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

use std::path::PathBuf;

use anyhow::Context;

use identity_iota::iota::block::output::AliasOutput;
use identity_iota::iota::IotaClientExt;
use identity_iota::iota::IotaDocument;
use identity_iota::iota::IotaIdentityClientExt;
use identity_iota::iota::NetworkName;
use identity_iota::storage::JwkDocumentExt;
use identity_iota::storage::Storage;
use identity_iota::verification::{MethodScope, VerificationMethod};

use identity_iota::verification::jws::JwsAlgorithm;
use iota_sdk::client::api::GetAddressesOptions;
use iota_sdk::client::node_api::indexer::query_parameters::QueryParameter;
use iota_sdk::client::secret::SecretManager;
use iota_sdk::client::Client;
use iota_sdk::crypto::keys::bip39;
use iota_sdk::types::block::address::Address;
use iota_sdk::types::block::address::Bech32Address;
use iota_sdk::types::block::address::Hrp;
use rand::distributions::DistString;
use serde_json::Value;

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
use identity_iota::storage::JwkStorage;

pub async fn generate(
    stronghold_ext_storage: &StrongholdExtStorage,
    key_type: KeyType,
    alg: JwsAlgorithm,
) -> Result<KeyId, ()> {
    let jwk_gen_output = stronghold_ext_storage.generate(key_type.clone(), alg).await.unwrap();
    info!(
        "Generated new {:?} key with key ID {:?}",
        &key_type.as_str(),
        &jwk_gen_output.key_id.as_str()
    );
    Ok(jwk_gen_output.key_id)
}

/// The unique identifier for the linked domain service.
pub const DOMAIN_LINKAGE_SERVICE_ID: &str = "linked-domain-service";

/// The unique identifier for the linked verifiable presentation service.
pub const VERIFIABLE_PRESENTATION_SERVICE_ID: &str = "linked-verifiable-presentation-service";

// #[tokio::test]
// async fn test() {
//     iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();
//     test_function().await;
// }

pub async fn test_function() {
    // The API endpoint of an IOTA node, e.g. Hornet.
    let api_endpoint: &str = "https://api.testnet.shimmer.network";

    // Create a new client to interact with the IOTA ledger.
    let client: Client = Client::builder()
        .with_primary_node(api_endpoint, None)
        .unwrap()
        .finish()
        .await
        .unwrap();

    let stronghold_password = config().secret_manager.stronghold_password.clone();

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(STRONGHOLD_PATH)
        .unwrap();

    // Create a `StrongholdStorage`.
    // `StrongholdStorage` creates internally a `SecretManager` that can be
    // referenced to avoid creating multiple instances around the same stronghold snapshot.
    let stronghold_ext_storage = StrongholdExtStorage::new(stronghold_adapter);

    let ed25519_key_id = generate(&stronghold_ext_storage, KeyType::new("Ed25519"), JwsAlgorithm::EdDSA)
        .await
        .unwrap();
    let es256_key_id = generate(&stronghold_ext_storage, KeyType::new("ES256"), JwsAlgorithm::ES256)
        .await
        .unwrap();

    let ed25519_jwk = stronghold_ext_storage
        .get_ed25519_public_key(&ed25519_key_id)
        .await
        .unwrap();

    let es256_jwk = stronghold_ext_storage
        .get_es256_public_key(&es256_key_id)
        .await
        .unwrap();

    // Create a DID document.
    let address: Bech32Address = get_address(&client, stronghold_ext_storage.as_secret_manager())
        .await
        .unwrap();
    println!("Address: {}", address);

    {
        let ledger_sponsoring_service = config().ledger_sponsoring_service.clone().unwrap();
        let access_key = ledger_sponsoring_service.access_key;
        let url = ledger_sponsoring_service.url;
        let authorization = ledger_sponsoring_service.authorization;

        let client = reqwest::Client::builder().build().unwrap();

        let json = serde_json::json!({
            "RequestSponsoring": {
                "access_key": access_key,
                "amount": 200000,
                "address": address.to_string()
            }
        });

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Authorization", authorization.parse().unwrap());

        let request = client.request(reqwest::Method::POST, url).headers(headers).json(&json);

        let response = request.send().await.unwrap();

        println!("Status: {}", response.status());

        std::thread::sleep(std::time::Duration::from_secs(15));
    }
    let address: Address = *address;
    println!("Address: {}", address);

    let network_name: NetworkName = client.network_name().await.unwrap();
    let document: IotaDocument = IotaDocument::new(&network_name);

    // Construct an Alias Output containing the DID document, with the wallet address
    // set as both the state controller and governor.
    let alias_output: AliasOutput = client.new_did_output(address, document, None).await.unwrap();

    // Publish the Alias Output and get the published DID document.
    let mut document: IotaDocument = client
        .publish_did_output(stronghold_ext_storage.as_secret_manager(), alias_output)
        .await
        .unwrap();
    let did = document.id().clone();

    let ed25519_verification_method: VerificationMethod =
        VerificationMethod::new_from_jwk(did.clone(), ed25519_jwk, None).unwrap();
    let es256_verification_method: VerificationMethod = VerificationMethod::new_from_jwk(did, es256_jwk, None).unwrap();

    document
        .insert_method(ed25519_verification_method, MethodScope::VerificationMethod)
        .unwrap();
    document
        .insert_method(es256_verification_method, MethodScope::VerificationMethod)
        .unwrap();

    // Resolve the latest output and update it with the given document.
    let alias_output: AliasOutput = client.update_did_output(document.clone()).await.unwrap();

    // Because the size of the DID document increased, we have to increase the allocated storage deposit.
    // This increases the deposit amount to the new minimum.
    let rent_structure: RentStructure = client.get_rent_structure().await.unwrap();
    let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
        .with_minimum_storage_deposit(rent_structure)
        .finish()
        .unwrap();

    // Publish the updated Alias Output.
    let updated: IotaDocument = client
        .publish_did_output(stronghold_ext_storage.as_secret_manager(), alias_output)
        .await
        .unwrap();
    println!("Updated DID document: {updated:#}");
}

/// Initialize the identity state.
pub async fn initialize(state: &IdentityState, subject: Arc<dyn Subject>) {
    info!("Initializing ...");

    // Only consider updateable DID methods.
    let did_methods = config()
        .did_methods
        .clone()
        .into_iter()
        .filter(|(did_method, _)| did_method.is_updateable())
        .collect::<Vec<_>>();

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
                    Ok(Some(_document_exists)) => {
                        if *enabled {
                            DocumentCommand::SetStatus {
                                document_id: document_id.clone(),
                                status: Status::SignAndValidate,
                            }
                        } else {
                            DocumentCommand::SetStatus {
                                document_id: document_id.clone(),
                                status: Status::Disabled,
                            }
                        }
                    }
                    // If the DID document does not exist yet, then it needs to be created.
                    _document_does_not_exist => {
                        if *enabled {
                            DocumentCommand::CreateDocument {
                                document_id: document_id.clone(),
                                status: Status::SignAndValidate,
                            }
                        } else {
                            return Err(format!("DID Document for `{did_method}` does not exist"));
                        }
                    }
                };

                info!("Executing command now: {:#?}", command);

                if command_handler(&document_id, &state.command.document, command)
                    .await
                    .is_err()
                {
                    warn!("5: Failed to Set status `{did_method}`");
                }

                info!("C: here");

                let stronghold_storage = stronghold_storage().await;

                let public_key_jwk: identity_iota::verification::jwk::Jwk = stronghold_storage
                    .get_ed25519_public_key(&KeyId::new(ED25519_KEY_ID))
                    .await
                    .unwrap();

                let command = DocumentCommand::AddPublicKeyJwk {
                    document_id: document_id.clone(),
                    public_key_jwk,
                };

                if command_handler(&document_id, &state.command.document, command)
                    .await
                    .is_err()
                {
                    warn!("5: Failed to Set status `{did_method}`");
                }

                info!("D: here");

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
        .filter(|document| document.status != Status::Disabled)
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
                            info!("document_id: {}", document_id);
                            let did_method = SupportedDidMethod::from_str(&document_id).unwrap();
                            let service = service.clone();

                            let command = match document.status {
                                Status::Disabled => {
                                    info!("I: Removing service: {document_id}");
                                    DocumentCommand::RemoveService {
                                        service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                    }
                                }
                                Status::SignAndValidate | Status::ValidateOnly => {
                                    info!("II: Adding service: {document_id}");
                                    DocumentCommand::AddService {
                                        service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
                                        service,
                                    }
                                }
                            };

                            info!("III: here");

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

    info!("Publish all documents");

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
                        document_id: document_id.clone(),
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
