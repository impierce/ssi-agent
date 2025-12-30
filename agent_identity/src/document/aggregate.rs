use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::services::IdentityServices;
use agent_secret_manager::managed_key::aggregate::SigningAlgorithm;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use agent_shared::config::{config_mut, SupportedDidMethod};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::document::CoreDocument;
use identity_iota::iota::rebased::client::{
    get_object_id_from_did, IdentityClient, IdentityClientReadOnly, PublishDidDocument,
};
use identity_iota::iota::rebased::migration::{ControllerToken, Identity, OnChainIdentity};
use identity_iota::iota::{rebased, IotaDID};
use identity_iota::storage::{Storage, StorageSigner};
use identity_iota::{
    iota::IotaDocument,
    verification::{MethodScope, MethodType, VerificationMethod},
};
use identity_storage::KeyId;
use identity_storage::{JwkStorage, KeyIdStorage};
use iota_sdk::types::base_types::IotaAddress;
use iota_sdk::{IotaClient, IotaClientBuilder};
use jsonwebtoken::Algorithm;
use product_common::core_client::CoreClient as _;
use product_common::gas_station::GasStationOptions;
use product_common::network_name::NetworkName;
use product_common::transaction::TransactionBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ssi_dids::DIDMethod;
use ssi_dids::Source;
use std::collections::HashMap;
use std::{collections::BTreeMap, sync::Arc};
use tracing::{debug, info, warn};
use url::Url;

// TODO: look into a more appropriate value for the minimum gas budget. This current value of `50_000_000` as adopted
// from examples from the IOTA identity library.
/// Minimum gas budget for publishing a DID Document on the IOTA ledger.
const MIN_GAS_BUDGET: u64 = 50_000_000;

/// Metadata for IOTA-based DID Documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct IotaMetadata {
    pub wallet_address: IotaAddress,
    pub is_funded: bool,
    pub balance: u64,
    pub is_published: bool,
    pub is_deactivated: bool,
    pub explorer_url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Status {
    SignAndValidate,
    // TODO: Make a distinction between enabling both signing AND validation and just validation.
    // ValidateOnly,
    #[default]
    Disabled,
}

// TODO: `Document` most likely should not be an Aggregate, but rather a Read Model that is built from events emitted by other Aggregates, such as `ManagedKey` and `Service`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    #[serde(rename = "id")]
    pub document_id: String,
    pub document: Option<CoreDocument>,
    pub did_method: Option<SupportedDidMethod>,
    // Applicable only for DID documents whose methods mandate a fixed verification algorithm,
    // such as `did:key` and `did:jwk`.
    pub with_fixed_algorithm: Option<Algorithm>,
    // Applicable only for DID methods that are based on the IOTA ledger.
    pub iota_metadata: Option<IotaMetadata>,
    pub verification_method_ids: HashMap<String, DIDUrl>,
    pub status: Status,
}

#[async_trait]
impl Aggregate for Document {
    type Command = DocumentCommand;
    type Event = DocumentEvent;
    type Error = DocumentError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "document".to_string()
    }

    // TODO: Most of how these commands are handled is not Domain logic, but rather Application logic, so it should be moved
    // to the Application layer. The Aggregate should only handle the Domain logic, such as creating a new Document, updating public keys, etc.
    // The Application layer should handle the specifics of how to create a Document based on the DID method, how to publish it, etc.
    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use DocumentCommand::*;
        use DocumentError::*;
        use DocumentEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateDocument {
                document_id,
                did_method,
                with_fixed_algorithm,
            } => {
                let subject = &services.subject;
                let stronghold_storage = &subject.stronghold_storage;

                let mut iota_metadata = self.iota_metadata.clone().unwrap_or_default();

                let document = match &did_method {
                    SupportedDidMethod::Iota | SupportedDidMethod::IotaDev | SupportedDidMethod::IotaTest => {
                        // Retrieve the network name associated with the DID method.
                        let network_name = did_method
                            .network_name()
                            .and_then(|network_name| NetworkName::try_from(network_name).ok())
                            .ok_or(MissingNetworkNameError(did_method))?;

                        let key_id = config().secret_manager.issuer_eddsa_key_id.clone();

                        let public_key_jwk = stronghold_storage
                            .get_ed25519_public_key(&key_id)
                            .await
                            .map_err(|err| GenericError(err.to_string()))?;

                        let storage = &Storage::new(stronghold_storage.clone(), stronghold_storage.clone());

                        // Create a signer for the IOTA client.
                        // This signer is used to sign the transactions that are sent to the IOTA ledger.
                        let signer = StorageSigner::new(storage, key_id, public_key_jwk.clone());

                        // The API endpoint of an IOTA node
                        let api_endpoint = did_method
                            .api_endpoint()
                            .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                        // Create a new IOTA client to interact with the IOTA ledger.
                        let iota_client = get_iota_client(api_endpoint).await?;

                        let read_only_client = IdentityClientReadOnly::new(iota_client.clone())
                            .await
                            .map_err(|err| GenericError(err.to_string()))?;

                        // Create an `IdentityClient` instance.
                        // This client is used to interact with the IOTA identity ledger.
                        // It is used to publish the DID Document and to resolve it later.
                        let identity_client = IdentityClient::new(read_only_client, signer)
                            .await
                            .map_err(|err| GenericError(err.to_string()))?;

                        // Retrieve the wallet address from the identity client.
                        let wallet_address = identity_client.sender_address();

                        let balance = iota_client
                            .coin_read_api()
                            .get_balance(wallet_address, None)
                            .await
                            .map_err(|err| GenericError(err.to_string()))?
                            .total_balance;

                        // TODO: This is a temporary solution to ensure that the wallet address is set in the configuration.
                        config_mut().iota_address = Some(wallet_address.to_string());

                        info!("Current {network_name} Address: `{wallet_address}`");

                        iota_metadata.wallet_address = wallet_address;
                        iota_metadata.balance = balance as u64;
                        iota_metadata.is_funded = balance > MIN_GAS_BUDGET as u128;

                        let document = self.document.clone().map(IotaDocument::from).unwrap_or_else(|| {
                            info!("Creating a new document for DID method `{did_method}`");

                            // Create a new 'blank' DID Document.
                            IotaDocument::new(&network_name)
                        });

                        let document = if config().iota_sponsoring_service_url.is_none() {
                            info!("Testing whether a DID Document can be published...");

                            // This code block is doing a dummy publish to ensure that the DID Document is created and can
                            // be published/updated later. It uses a gas budget of 0 to avoid actually publishing the
                            // document.
                            let document = match identity_client.publish_did_document_update(document.clone(), 0).await
                            {
                                // This match arm can never be reached, because we use a gas budget of 0.
                                Ok(document) => document,
                                // This error occurs when the DID Document is not published yet. We will not return an
                                // error because the `PublishDocument` command will handle the actual publishing if the
                                // funds are sufficient.
                                Err(identity_iota::iota::rebased::Error::DIDResolutionError(_err)) => {
                                    warn!("Document is not published yet.");

                                    document
                                }
                                // This error occurs when the `identity_client` has no control over the DID Document that
                                // has been stored in the aggregate instance. This usually means that the current keys
                                // stored in the KMS have been updated between boots. We throw an error here indicating
                                // that the original KMS needs to be used or that the database needs to be wiped.
                                // TODO: implement KMS migration.
                                Err(identity_iota::iota::rebased::Error::Identity(err))
                                    if err.contains("address") && err.contains("has no control over Identity") =>
                                {
                                    warn!("No control over the identity, as no matching keys were found in the key storage: {err}");

                                    return Err(DocumentError::IotaControllerError(
                                        identity_iota::iota::rebased::Error::Identity(err),
                                    ));
                                }
                                // This error is to be expected because we use a gas budget of 0.
                                Err(identity_iota::iota::rebased::Error::TransactionUnexpectedResponse(err))
                                    if err.contains("Gas budget: 0 is lower than min") =>
                                {
                                    info!("Document can be published or updated later if the funds are sufficient.");

                                    document
                                }
                                // Any other error is unexpected and should be handled.
                                Err(err) => return Err(DocumentError::IotaIdentityError(err)),
                            };

                            document
                        } else {
                            info!("Sponsoring service configured, skipping dummy publish test.");

                            document
                        };

                        info!("DID Document created: {document:#?}");

                        document.into()
                    }
                    SupportedDidMethod::Web => {
                        let origin = config().public_url.origin();

                        info!("Origin: {}", &origin.ascii_serialization());

                        let (_scheme, host, port) = match origin {
                            url::Origin::Tuple(ref scheme, ref host, ref port) => (scheme, host, port),
                            url::Origin::Opaque(_) => {
                                return Err(OpaqueOriginError);
                            }
                        };

                        // IP addresses are not allowed
                        if matches!(host, url::Host::Ipv4(_) | url::Host::Ipv6(_)) {
                            return Err(HostError);
                        }

                        // Omit default HTTPS port
                        let host_port_encoded = match port {
                            443 => host.to_string(),
                            _ => urlencoding::encode(format!("{host}:{port}").as_str()).to_string(),
                        };

                        let controller = format!("did:web:{host_port_encoded}")
                            .parse::<CoreDID>()
                            .map_err(|err| InvalidDidError(err.to_string()))?;

                        // Patch the generated DID document since it's not according to spec.
                        let properties = get_properties(MethodType::JSON_WEB_KEY_2020);

                        CoreDocument::builder(properties)
                            .id(controller)
                            .build()
                            .map_err(ProduceDocumentError)?
                    }
                    SupportedDidMethod::Jwk | SupportedDidMethod::Key => {
                        let algorithm = with_fixed_algorithm.ok_or(MissingFixedAlgorithmError(did_method))?;
                        let key_id = match algorithm {
                            Algorithm::EdDSA => config().secret_manager.issuer_eddsa_key_id.clone(),
                            Algorithm::ES256 => config().secret_manager.issuer_es256_key_id.clone(),
                            algorithm => return Err(UnsupportedSigningAlgorithmError(algorithm)),
                        };

                        // Retrieve the public key from Stronghold.
                        let public_key_jwk = subject
                            .get_public_key(key_id.clone(), &algorithm)
                            .await
                            .map_err(|err| MissingKeyError(err.to_string()))?;

                        // Generate a DID from the public key.
                        let controller = serde_json::from_value::<ssi_jwk::JWK>(json!(public_key_jwk))
                            .ok()
                            .and_then(|jwk| match did_method {
                                SupportedDidMethod::Jwk => did_jwk_extern::DIDJWK.generate(&Source::Key(&jwk)),
                                SupportedDidMethod::Key => did_key_extern::DIDKey.generate(&Source::Key(&jwk)),
                                _ => None,
                            })
                            .and_then(|did| did.parse().ok())
                            .ok_or(GenerateDidError(key_id))?;

                        CoreDocument::builder(Default::default())
                            .id(controller)
                            .build()
                            .map_err(ProduceDocumentError)?
                    }
                };

                let status = Status::SignAndValidate;

                let iota_metadata = (did_method == SupportedDidMethod::Iota
                    || did_method == SupportedDidMethod::IotaDev
                    || did_method == SupportedDidMethod::IotaTest)
                    .then_some(iota_metadata);

                Ok(vec![DocumentCreated {
                    document_id,
                    did_method,
                    status,
                    document,
                    with_fixed_algorithm,
                    iota_metadata,
                }])
            }
            UpdateDocumentStatus { status } => Ok(vec![DocumentStatusUpdated {
                document_id: self.document_id.clone(),
                status,
            }]),
            AddVerificationMethod {
                key_id,
                signing_algorithm,
            } => {
                let subject = &services.subject;
                let stronghold_storage = &subject.stronghold_storage;

                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let did = document.id().clone();

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                let key_id = KeyId::new(&key_id);

                let jwk = match signing_algorithm {
                    SigningAlgorithm::EdDSA => stronghold_storage.get_ed25519_public_key(&key_id).await.unwrap(),
                    SigningAlgorithm::ES256 => stronghold_storage.get_es256_public_key(&key_id).await.unwrap(),
                };

                let verification_method = VerificationMethod::new_from_jwk(
                    did.clone(),
                    jwk,
                    (did_method == SupportedDidMethod::Key)
                        .then_some(did.method_id())
                        .or(did_method.fragment()),
                )
                .map_err(|err| VerificationMethodBuilderError(err.to_string()))?;

                let mut verification_method_ids = self.verification_method_ids.clone();
                verification_method_ids.insert(key_id.to_string(), verification_method.id().clone());

                document
                    .insert_method(verification_method, MethodScope::VerificationMethod) // TODO: add relationships, also TODO: adjust KID insertion elsewhere
                    .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                Ok(vec![VerificationMethodAdded {
                    document_id: self.document_id.clone(),
                    verification_method_ids,
                    document,
                }])
            }
            RemoveVerificationMethod { key_id } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                let mut verification_method_ids = self.verification_method_ids.clone();
                let verification_method_id = verification_method_ids.remove(&key_id.to_string()).unwrap();

                document.remove_method(&verification_method_id);

                Ok(vec![VerificationMethodRemoved {
                    document_id: self.document_id.clone(),
                    document,
                    verification_method_ids,
                }])
            }
            AddService {
                service_id,
                mut service,
            } => {
                let document_id = self.document_id.clone();
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let subject_did = document.id();

                // Set the service ID.
                format!("{subject_did}#{service_id}")
                    .parse::<DIDUrl>()
                    .ok()
                    .and_then(|service_id| service.set_id(service_id).ok())
                    .ok_or_else(|| InvalidDidError(service_id.to_string()))?;

                // Overwrite the service if it already exists.
                document.remove_service(service.id());
                document
                    .insert_service(*service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document_id, document }])
            }
            PublishDocument => {
                let mut document: IotaDocument = self.document.clone().ok_or(MissingDocumentError)?.into();

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                // The API endpoint of an IOTA node
                let api_endpoint = did_method
                    .api_endpoint()
                    .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                // Create a new IOTA client to interact with the IOTA ledger.
                let iota_client = get_iota_client(api_endpoint).await?;

                let stronghold_storage = &services.subject.stronghold_storage;

                let key_id = config().secret_manager.issuer_eddsa_key_id.clone();

                let public_key_jwk = stronghold_storage
                    .get_ed25519_public_key(&key_id)
                    .await
                    .map_err(|err| GenericError(err.to_string()))?;

                let storage = &Storage::new(stronghold_storage.clone(), stronghold_storage.clone());

                // Create a signer for the IOTA client.
                // This signer is used to sign the transactions that are sent to the IOTA ledger.
                let signer = StorageSigner::new(storage, key_id, public_key_jwk.clone());

                let read_only_client = IdentityClientReadOnly::new(iota_client.clone())
                    .await
                    .map_err(|err| GenericError(err.to_string()))?;

                // Create an `IdentityClient` instance.
                // This client is used to interact with the IOTA identity ledger.
                // It is used to publish the DID Document and to resolve it later.
                let identity_client = IdentityClient::new(read_only_client, signer)
                    .await
                    .map_err(|err| GenericError(err.to_string()))?;

                let iota_metadata = self.iota_metadata.clone().unwrap_or_default();

                if iota_metadata.is_published {
                    let published_document = identity_client
                        .resolve_did(document.id())
                        .await
                        .map_err(DocumentError::IotaIdentityError)?;

                    if published_document.core_document() == document.core_document() {
                        info!("Document instance does not contain any updates, skipping publishing.");
                        return Ok(vec![]);
                    }
                }

                // Retrieve the wallet address from the identity client.
                let wallet_address = identity_client.sender_address();

                let network_name = did_method
                    .network_name()
                    .and_then(|network_name| NetworkName::try_from(network_name).ok())
                    .ok_or(MissingNetworkNameError(did_method))?;

                let iota_sponsoring_service_url = config().iota_sponsoring_service_url.clone();
                let iota_sponsoring_service_auth = config().iota_sponsoring_service_auth.clone();

                if !iota_metadata.is_funded && iota_sponsoring_service_url.is_none() {
                    warn!(
                        "Skipping publishing DID Document for DID method `{did_method}` because it is not sufficiently funded",  
                    );

                    let did = self
                        .document
                        .as_ref()
                        .map(|document| IotaDocument::from(document.to_owned()).id().to_owned())
                        .unwrap_or_else(|| IotaDID::placeholder(&network_name));

                    let document = CoreDocument::from(IotaDocument::new_with_id(did));

                    return Ok(vec![DocumentDeleted {
                        document_id: self.document_id.clone(),
                        document,
                    }]);
                }

                let mut iota_metadata = self.iota_metadata.clone().unwrap_or_default();

                if !iota_metadata.is_published {
                    info!("Publishing DID Document for the first time...");

                    document = publish_did_document(
                        &identity_client,
                        document.clone(),
                        wallet_address,
                        MIN_GAS_BUDGET,
                        &iota_sponsoring_service_url,
                        iota_sponsoring_service_auth.as_deref(),
                    )
                    .await?;

                    iota_metadata.is_published = true;
                    iota_metadata.created_at = document.metadata.created.map(|created| created.to_string());
                } else {
                    info!("Updating existing DID Document...");

                    // Update the existing DID Document.
                    match self.status {
                        // This status indicates that the DID Document update is ready to be published to the IOTA ledger.
                        Status::SignAndValidate => {
                            info!("Updating DID Document with status: SignAndValidate");

                            update_did_document(
                                &identity_client,
                                document.clone(),
                                MIN_GAS_BUDGET,
                                &iota_sponsoring_service_url,
                                iota_sponsoring_service_auth.as_deref(),
                            )
                            .await?;

                            iota_metadata.is_deactivated = false;
                        }
                        Status::Disabled => {
                            // This status indicates that the DID Document should be deactivated.

                            info!("Deactivating DID Document with status: Disabled");

                            deactivate_did(
                                &identity_client,
                                document.clone(),
                                MIN_GAS_BUDGET,
                                &iota_sponsoring_service_url,
                                iota_sponsoring_service_auth.as_deref(),
                            )
                            .await?;

                            iota_metadata.is_deactivated = true;
                        }
                    };
                };

                let document = identity_client
                    .resolve_did(document.id())
                    .await
                    .map_err(|err| GenericError(err.to_string()))?;

                info!("DID Document after publishing: {document:#?}");

                let balance = iota_client
                    .coin_read_api()
                    .get_balance(wallet_address, None)
                    .await
                    .map_err(|err| GenericError(err.to_string()))?
                    .total_balance;

                iota_metadata.is_funded = balance > MIN_GAS_BUDGET as u128;
                iota_metadata.balance = balance as u64;
                iota_metadata.updated_at = document.metadata.updated.map(|updated| updated.to_string());

                info!("Updated IOTA Metadata: {iota_metadata:#?}");

                iota_metadata.explorer_url = Some(format!(
                    "https://explorer.iota.org/object/{}?network={}",
                    document.id().tag_str(),
                    if did_method == SupportedDidMethod::IotaDev {
                        "devnet"
                    } else if did_method == SupportedDidMethod::IotaTest {
                        "testnet"
                    } else {
                        "mainnet"
                    }
                ));

                info!("Explorer URL: {:?}", iota_metadata.explorer_url);

                Ok(vec![DocumentPublished {
                    document_id: self.document_id.clone(),
                    document: CoreDocument::from(document),
                    iota_metadata: Some(iota_metadata),
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            DocumentCreated {
                document_id,
                did_method,
                status,
                document,
                with_fixed_algorithm,
                iota_metadata,
            } => {
                self.document_id = document_id;
                self.did_method.replace(did_method);
                self.status = status;
                self.document.replace(document);
                self.with_fixed_algorithm = with_fixed_algorithm;
                self.iota_metadata = iota_metadata;
            }
            PublicKeyUpdated { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            DocumentStatusUpdated { document_id, status } => {
                self.document_id = document_id;
                self.status = status;
            }
            ServiceAdded { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            VerificationMethodAdded {
                document_id,
                document,
                verification_method_ids,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.verification_method_ids = verification_method_ids;
            }
            VerificationMethodRemoved {
                document_id,
                document,
                verification_method_ids,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.verification_method_ids = verification_method_ids;
            }
            DocumentPublished {
                document_id,
                document,
                iota_metadata,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.iota_metadata = iota_metadata;
            }
            DocumentDeleted { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
        }
    }
}

/// Helper function to retrieve the On-Chain Identity (OCI) and Controller Token.
/// This code is extracted from `IdentityCient::publish_did_document_update` and
/// `IdentityCient::deactivate_did_output` so that it can be used to update and
/// deactivate DID Documents through an IOTA Gas Station.
async fn get_oci_and_controller_token<K, I>(
    identity_client: &IdentityClient<StorageSigner<'_, K, I>>,
    document: &IotaDocument,
) -> Result<(OnChainIdentity, ControllerToken), rebased::Error>
where
    K: JwkStorage,
    I: KeyIdStorage,
{
    let oci = if let Identity::FullFledged(value) = identity_client
        .get_identity(get_object_id_from_did(document.id())?)
        .await?
    {
        value
    } else {
        return Err(rebased::Error::Identity(
            "only new identities can be updated".to_string(),
        ));
    };

    let controller_token = oci.get_controller_token(identity_client).await?.ok_or_else(|| {
        rebased::Error::Identity(format!(
            "address {} has no control over Identity {}",
            identity_client.sender_address(),
            oci.id()
        ))
    })?;

    Ok((oci, controller_token))
}

async fn publish_did_document<K, I>(
    identity_client: &IdentityClient<StorageSigner<'_, K, I>>,
    document: IotaDocument,
    wallet_address: IotaAddress,
    gas_budget: u64,
    iota_sponsoring_service_url: &Option<Url>,
    iota_sponsoring_service_auth: Option<&str>,
) -> Result<IotaDocument, DocumentError>
where
    K: JwkStorage,
    I: KeyIdStorage,
{
    let document = if let Some(iota_sponsoring_service_url) = iota_sponsoring_service_url {
        info!("Publishing DID Document using IOTA Gas Station...");

        TransactionBuilder::new(PublishDidDocument::new(document, wallet_address))
            .with_gas_budget(MIN_GAS_BUDGET)
            .execute_with_gas_station(
                identity_client,
                iota_sponsoring_service_url.as_str(),
                iota_sponsoring_service_auth.map(|auth| GasStationOptions::default().with_auth_token(auth)),
            )
            .await
            .map_err(|err| DocumentError::IotaPublishDocumentError(err.to_string()))?
            .output
    } else {
        info!("Publishing DID Document...");

        identity_client
            .publish_did_document(document)
            .with_gas_budget(gas_budget)
            .build_and_execute(identity_client)
            .await
            .map_err(|err| DocumentError::IotaPublishDocumentError(err.to_string()))?
            .output
    };

    Ok(document)
}

async fn update_did_document<K, I>(
    identity_client: &IdentityClient<StorageSigner<'_, K, I>>,
    document: IotaDocument,
    gas_budget: u64,
    iota_sponsoring_service_url: &Option<Url>,
    iota_sponsoring_service_auth: Option<&str>,
) -> Result<(), DocumentError>
where
    K: JwkStorage,
    I: KeyIdStorage,
{
    if let Some(iota_sponsoring_service_url) = iota_sponsoring_service_url {
        info!("Updating DID Document using IOTA Gas Station...");

        let (mut oci, controller_token) = get_oci_and_controller_token(identity_client, &document).await?;

        oci.update_did_document(document, &controller_token)
            .finish(identity_client)
            .await?
            .with_gas_budget(gas_budget)
            .execute_with_gas_station(
                identity_client,
                iota_sponsoring_service_url.as_str(),
                iota_sponsoring_service_auth.map(|auth| GasStationOptions::default().with_auth_token(auth)),
            )
            .await
            .map_err(|err| DocumentError::IotaUpdateDocumentError(err.to_string()))?;
    } else {
        info!("Updating DID Document...");

        identity_client
            .publish_did_document_update(document, MIN_GAS_BUDGET)
            .await
            .map_err(|err| DocumentError::IotaUpdateDocumentError(err.to_string()))?;
    }
    Ok(())
}

async fn deactivate_did<K, I>(
    identity_client: &IdentityClient<StorageSigner<'_, K, I>>,
    document: IotaDocument,
    gas_budget: u64,
    iota_sponsoring_service_url: &Option<Url>,
    iota_sponsoring_service_auth: Option<&str>,
) -> Result<(), DocumentError>
where
    K: JwkStorage,
    I: KeyIdStorage,
{
    if let Some(iota_sponsoring_service_url) = iota_sponsoring_service_url {
        info!("Deactivating DID using IOTA Gas Station...");

        let (mut oci, controller_token) = get_oci_and_controller_token(identity_client, &document).await?;

        oci.deactivate_did(&controller_token)
            .finish(identity_client)
            .await?
            .with_gas_budget(gas_budget)
            .execute_with_gas_station(
                identity_client,
                iota_sponsoring_service_url.as_str(),
                iota_sponsoring_service_auth.map(|auth| GasStationOptions::default().with_auth_token(auth)),
            )
            .await
            .map_err(|err| DocumentError::IotaDeactivateDidError(err.to_string()))?;
    } else {
        info!("Deactivating DID...");

        identity_client
            .deactivate_did_output(document.id(), MIN_GAS_BUDGET)
            .await
            .map_err(|err| DocumentError::IotaDeactivateDidError(err.to_string()))?;
    }

    Ok(())
}

pub async fn get_iota_client(api_endpoint: &str) -> Result<IotaClient, DocumentError> {
    let mut iota_client_builder = IotaClientBuilder::default();

    if let Some(iota_node_url) = config().iota_node_url.clone() {
        iota_client_builder = iota_client_builder.ws_url(iota_node_url);

        if let Some(iota_node_url_auth) = config().iota_node_username.clone() {
            if let Some(iota_node_password) = config().iota_node_password.clone() {
                iota_client_builder = iota_client_builder.basic_auth(iota_node_url_auth, iota_node_password);
            } else {
                warn!("No IOTA node URL password configured in the application configuration.");
            }
        } else {
            warn!("No IOTA node URL authentication configured in the application configuration.");
        }
    } else {
        warn!("No IOTA node URL configured in the application configuration.");
    }

    let iota_client = iota_client_builder
        .build(api_endpoint)
        .await
        .map_err(|err| DocumentError::IotaClientBuilderError(err.to_string()))?;

    Ok(iota_client)
}

// TODO: Can we remove this? It does not seem to be required: https://w3c-ccg.github.io/did-method-web/#key-material-and-document-handling
pub fn get_properties(method_type: MethodType) -> BTreeMap<String, serde_json::Value> {
    let mut properties = BTreeMap::new();
    properties.insert(
        "@context".to_string(),
        match method_type.as_str() {
            "Ed25519VerificationKey2018" => json!([
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/suites/ed25519-2018/v1"
            ]),
            "JsonWebKey2020" => json!([
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/suites/jws-2020/v1"
            ]),
            _ => unimplemented!("Unsupported method type"),
        },
    );
    properties
}

#[cfg(test)]
pub mod document_tests {
    use crate::state::DOMAIN_LINKAGE_SERVICE_ID;

    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use identity_document::service::Service;
    use rstest::rstest;

    type DocumentTestFramework = TestFramework<Document>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_document(document_id: String, did_method: SupportedDidMethod, document: CoreDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id: document_id.clone(),
                did_method,
                with_fixed_algorithm: None,
            })
            .then_expect_events(vec![DocumentEvent::DocumentCreated {
                document_id,
                did_method,
                document,
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
                iota_metadata: None,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_service(
        document_id: String,
        did_method: SupportedDidMethod,
        document: CoreDocument,
        domain_linkage_service: Service,
        document_with_multiple_verification_methods: CoreDocument,
        document_with_domain_linkage_service: CoreDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                DocumentEvent::DocumentCreated {
                    document_id: document_id.clone(),
                    did_method,
                    document,
                    status: Status::SignAndValidate,
                    with_fixed_algorithm: None,
                    iota_metadata: None,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_multiple_verification_methods,
                },
            ])
            .when(DocumentCommand::AddService {
                service: Box::new(domain_linkage_service),
                service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
            })
            .then_expect_events(vec![DocumentEvent::ServiceAdded {
                document_id: document_id.clone(),
                document: document_with_domain_linkage_service,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_set_status(
        document_id: String,
        did_method: SupportedDidMethod,
        document: CoreDocument,
        document_with_multiple_verification_methods: CoreDocument,
        document_with_domain_linkage_service: CoreDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                DocumentEvent::DocumentCreated {
                    document_id: document_id.clone(),
                    did_method,
                    document,
                    status: Status::SignAndValidate,
                    with_fixed_algorithm: None,
                    iota_metadata: None,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_multiple_verification_methods.clone(),
                },
                DocumentEvent::ServiceAdded {
                    document_id: document_id.clone(),
                    document: document_with_domain_linkage_service,
                },
            ])
            .when(DocumentCommand::UpdateDocumentStatus {
                status: Status::Disabled,
            })
            .then_expect_events(vec![DocumentEvent::DocumentStatusUpdated {
                document_id,
                status: Status::Disabled,
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::get_properties;
    use crate::state::DOMAIN_LINKAGE_SERVICE_ID;
    use agent_shared::config::{config, SupportedDidMethod};
    use identity_core::convert::FromJson;
    use identity_did::CoreDID;
    use identity_document::{
        document::CoreDocument,
        service::{Service, ServiceEndpoint},
    };
    use identity_iota::verification::jwk::Jwk;
    use identity_iota::verification::{MethodData, MethodScope, MethodType, VerificationMethod};
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn document_id() -> String {
        "document_id".to_string()
    }

    #[fixture]
    pub fn did_method() -> SupportedDidMethod {
        SupportedDidMethod::Web
    }

    #[fixture]
    pub fn document() -> CoreDocument {
        let controller: CoreDID = "did:web:my-domain.example.org".parse().unwrap();

        CoreDocument::builder(get_properties(MethodType::JSON_WEB_KEY_2020))
            .id(controller.clone())
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn es256_verification_method() -> VerificationMethod {
        VerificationMethod::builder(Default::default())
            .id(
                "did:web:my-domain.example.org#oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg"
                    .parse()
                    .unwrap(),
            )
            .controller("did:web:my-domain.example.org".parse().unwrap())
            .type_(MethodType::JSON_WEB_KEY_2020)
            .data(MethodData::PublicKeyJwk(
                Jwk::from_json_value(json!({
                    "kty": "EC",
                    "alg": "ES256",
                    "kid": "oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg",
                    "crv": "P-256",
                    "x": "Fmk13gO2SGLbuXeL24qJPHCNncnI6lBu6ZQL2EVZv4E",
                    "y": "fz2KvMhufzUpMeL9-K2re9fwA3mzg1bpfbceIQSuihY"
                }))
                .unwrap(),
            ))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn eddsa_verification_method() -> VerificationMethod {
        VerificationMethod::builder(Default::default())
            .id(
                "did:web:my-domain.example.org#bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0"
                    .parse()
                    .unwrap(),
            )
            .controller("did:web:my-domain.example.org".parse().unwrap())
            .type_(MethodType::JSON_WEB_KEY_2020)
            .data(MethodData::PublicKeyJwk(
                Jwk::from_json_value(json!({
                    "kty": "OKP",
                    "alg": "EdDSA",
                    "kid": "bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0",
                    "crv": "Ed25519",
                    "x": "GlnK9ePs802XxAglROQzoGurm9Qpv0IFPEbdMCILN_U"
                }))
                .unwrap(),
            ))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn both_verification_methods(
        es256_verification_method: VerificationMethod,
        eddsa_verification_method: VerificationMethod,
    ) -> Vec<VerificationMethod> {
        vec![es256_verification_method, eddsa_verification_method]
    }

    #[fixture]
    pub fn document_with_single_verification_method(
        mut document: CoreDocument,
        es256_verification_method: VerificationMethod,
    ) -> CoreDocument {
        document
            .insert_method(es256_verification_method, MethodScope::VerificationMethod)
            .unwrap();

        document
    }

    #[fixture]
    pub fn document_with_multiple_verification_methods(
        mut document: CoreDocument,
        both_verification_methods: Vec<VerificationMethod>,
    ) -> CoreDocument {
        for verification_method in both_verification_methods {
            document
                .insert_method(verification_method, MethodScope::VerificationMethod)
                .unwrap();
        }

        document
    }

    #[fixture]
    pub fn domain_linkage_service() -> Service {
        Service::builder(Default::default())
            .id(format!("did:web:my-domain.example.org#{DOMAIN_LINKAGE_SERVICE_ID}")
                .parse()
                .unwrap())
            .type_("LinkedDomains")
            .service_endpoint(
                ServiceEndpoint::from_json_value(json!({
                    "origins": [config().public_url.origin().ascii_serialization()],
                }))
                .unwrap(),
            )
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn document_with_domain_linkage_service(
        mut document_with_multiple_verification_methods: CoreDocument,
        domain_linkage_service: Service,
    ) -> CoreDocument {
        document_with_multiple_verification_methods
            .insert_service(domain_linkage_service)
            .unwrap();

        document_with_multiple_verification_methods
    }
}
