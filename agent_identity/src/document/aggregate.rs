use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::services::IdentityServices;
use agent_secret_manager::subject::StorageKey;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use agent_shared::config::{config_mut, SupportedDidMethod};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::document::CoreDocument;
use identity_iota::iota::rebased::client::{IdentityClient, IdentityClientReadOnly};
use identity_iota::storage::{Storage, StorageSigner};
use identity_iota::{
    iota::IotaDocument,
    verification::{MethodScope, MethodType, VerificationMethod},
};
use identity_storage::KeyId;
use iota_sdk::types::base_types::IotaAddress;
use iota_sdk::IotaClientBuilder;
use jsonwebtoken::Algorithm;
use product_common::network_name::NetworkName;
use secret_storage::Signer;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ssi_dids::DIDMethod;
use ssi_dids::Source;
use std::str::FromStr as _;
use std::{collections::BTreeMap, sync::Arc};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IotaMetadata {
    pub wallet_address: IotaAddress,
    pub funded: bool,
    pub balance: u64,
    pub explorer_url: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Status {
    SignAndValidate,
    // TODO: Make a distinction between enabling both signing AND validation and just validation.
    // ValidateOnly,
    #[default]
    Disabled,
}

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

                let mut iota_metadata = self.iota_metadata.clone().unwrap_or_else(|| IotaMetadata {
                    wallet_address: IotaAddress::default(),
                    funded: false,
                    balance: 0,
                    created: None,
                    updated: None,
                    explorer_url: None,
                });

                let document = match &did_method {
                    SupportedDidMethod::Iota | SupportedDidMethod::IotaDev => {
                        // The API endpoint of an IOTA node.
                        let api_endpoint = did_method
                            .api_endpoint()
                            .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                        // Retrieve the network name associated with the DID method.
                        let network_name = did_method.network_name().ok_or(MissingNetworkNameError(did_method))?;

                        // Build a new IOTA client to interact with the IOTA ledger.
                        let mut iota_client_builder = IotaClientBuilder::default();

                        if let Some(iota_node_url) = config().iota_node_url.clone() {
                            iota_client_builder = iota_client_builder.ws_url(iota_node_url);

                            if let Some(iota_node_url_auth) = config().iota_node_username.clone() {
                                if let Some(iota_node_password) = config().iota_node_password.clone() {
                                    iota_client_builder =
                                        iota_client_builder.basic_auth(iota_node_url_auth, iota_node_password);
                                } else {
                                    warn!("No IOTA node URL password configured in the application configuration.");
                                }
                            } else {
                                warn!("No IOTA node URL authentication configured in the application configuration.");
                            }
                        } else {
                            warn!("No IOTA node URL configured in the application configuration.");
                        }

                        let iota_client = iota_client_builder.build(api_endpoint).await.unwrap();

                        // FIXME!
                        let key_id = KeyId::new("ed25519-0");

                        let public_key_jwk = stronghold_storage.get_ed25519_public_key(&key_id).await.unwrap();

                        let storage = &Storage::new(stronghold_storage.clone(), stronghold_storage.clone());

                        let signer = StorageSigner::new(storage, key_id, public_key_jwk.clone());

                        let wallet_address = IotaAddress::from(&Signer::public_key(&signer).await.unwrap());
                        let balance = iota_client
                            .coin_read_api()
                            .get_balance(wallet_address, Some("0x2::iota::IOTA".to_string()))
                            .await
                            .unwrap()
                            .total_balance;

                        config_mut().iota_address = Some(wallet_address.to_string());

                        info!("Current {network_name} Address: `{wallet_address}`");

                        let read_only_client = IdentityClientReadOnly::new(iota_client.clone()).await.unwrap();
                        let identity_client = IdentityClient::new(read_only_client, signer).await.unwrap();

                        // Check if a DID Document already exists in the aggregate.
                        // If so, attempt to publish it to validate that the current wallet address is in control of it.
                        let document = if let Some(document) = self.document.clone().map(IotaDocument::from) {
                            let controller = document.id().clone();
                            info!("Found an existing controller for DID method `{did_method}`: `{controller}`");

                            // Create a new DID Document from scratch.
                            let document = IotaDocument::new_with_id(controller.clone());

                            // Update the DID Document output with the latest state.
                            let publish_result = identity_client
                                // FIXME: gas?
                                .publish_did_document_update(document.clone(), 50_000_000)
                                .await;

                            match publish_result {
                                // The current wallet address controls the existing DID Document.
                                Ok(document) => Some(document),
                                Err(test_publish_error) => {
                                    info!("Failed to publish existing DID Document: {test_publish_error:?}");

                                    match test_publish_error {
                                        // This specific error signifies that the current wallet address is NOT in
                                        // control of the DID Document found in the Aggregate.
                                        identity_iota::iota::rebased::Error::Identity(identity_error) => {
                                            warn!(identity_error);

                                            // We don't return an error here. Instead we assign `None` to `document` so
                                            // that later on a new DID Document will be created using the current
                                            // wallet address.
                                            None
                                        }
                                        identity_iota::iota::rebased::Error::DIDResolutionError(_error) => {
                                            // This error indicates that the DID Document could not be resolved.
                                            // We don't return an error here. Instead we assign `None` to `document` so
                                            // that later on a new DID Document will be created using the current
                                            // wallet address.
                                            None
                                        }
                                        other_test_publish_error => {
                                            return Err(IotaIdentityError(other_test_publish_error));
                                        }
                                    }
                                }
                            }
                        } else {
                            None
                        };

                        iota_metadata.wallet_address = wallet_address;

                        let document = if let Some(document) = document {
                            iota_metadata.funded = true;
                            iota_metadata.balance = balance as u64;

                            iota_metadata.explorer_url = Some(format!(
                                "https://explorer.iota.org/object/{}?network={}",
                                document.id().tag_str(),
                                if did_method == SupportedDidMethod::IotaDev {
                                    "devnet"
                                } else {
                                    "mainnet"
                                }
                            ));
                            iota_metadata.created = document.metadata.created.map(|created| created.to_string());
                            iota_metadata.updated = document.metadata.updated.map(|updated| updated.to_string());

                            // Return the DID Document that was already stored in the Aggregate now we validated that
                            // the current Stronghold storage is in control of it.
                            document
                        } else {
                            // If there was no DID Document stored in the Aggregate yet, or the current Stronghold
                            // storage is not in control of it, then we create a completely new controller and DID Document.
                            info!("Creating a new controller for DID method `{did_method}`");

                            // Create a new 'blank' DID Document.
                            let document = IotaDocument::new(&NetworkName::from_str(network_name).unwrap());

                            // Update the DID Document output with the latest state.
                            let publish_result = identity_client
                                // FIXME: gas?
                                .publish_did_document(document.clone())
                                .with_gas_budget(50_000_000)
                                .build_and_execute(&identity_client)
                                .await;

                            match publish_result {
                                // Creating and publishing the new DID Document was successful.
                                Ok(transaction) => {
                                    iota_metadata.funded = true;
                                    iota_metadata.balance = balance as u64;

                                    let document = transaction.output;
                                    iota_metadata.explorer_url = Some(format!(
                                        "https://explorer.iota.org/object/{}?network={}",
                                        document.id().tag_str(),
                                        if did_method == SupportedDidMethod::IotaDev {
                                            "devnet"
                                        } else {
                                            "mainnet"
                                        }
                                    ));
                                    iota_metadata.created =
                                        document.metadata.created.map(|created| created.to_string());
                                    iota_metadata.updated =
                                        document.metadata.updated.map(|updated| updated.to_string());

                                    info!("Created DID Document 1: {document:#}");
                                    document
                                }
                                // This error indicates that the Wallet Address does not have sufficient funds and
                                // therefore we need to throw an explixit `InsufficientDepositError` error message.
                                Err(product_common::error::Error::GasIssue(error)) => {
                                    warn!(error, "Insufficient funds to publish DID Document");
                                    iota_metadata.funded = false;
                                    iota_metadata.balance = balance as u64;
                                    iota_metadata.created = None;
                                    iota_metadata.updated = None;

                                    // return Err(InsufficientDepositError(
                                    //     network_name.to_string(),
                                    //     wallet_address.to_string(),
                                    // ));
                                    let status = Status::SignAndValidate;

                                    return Ok(vec![DocumentCreated {
                                        document_id,
                                        did_method,
                                        status,
                                        document: document.into(),
                                        with_fixed_algorithm,
                                        iota_metadata: Some(iota_metadata),
                                    }]);
                                }
                                Err(other_error) => return Err(IotaProductCommonError(other_error)),
                            }
                        };

                        warn!("HHEHRERERERERER4");
                        // Publish the updated Alias Output.
                        let updated_document = identity_client
                            .publish_did_document_update(document.clone(), 50_000_000)
                            .await
                            .map(CoreDocument::from)
                            .unwrap();

                        info!("Created DID Document 2: {updated_document:#}");

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

                let iota_metadata = if let SupportedDidMethod::Iota | SupportedDidMethod::IotaDev = did_method {
                    Some(iota_metadata)
                } else {
                    None
                };

                Ok(vec![DocumentCreated {
                    document_id,
                    did_method,
                    status,
                    document,
                    with_fixed_algorithm,
                    iota_metadata,
                }])
            }
            UpdatePublicKeys {
                // TODO: decide whether the public keys should be supplied through the command or not.
                public_key_jwks: _,
            } => {
                if let Some(iota_metadata) = self.iota_metadata.as_ref() {
                    if !iota_metadata.funded {
                        warn!(
                            "Skipping updating public keys for DID method `{}` because it is not funded",
                            self.did_method.as_ref().unwrap()
                        );
                        return Ok(vec![]);
                    }
                }

                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let did = document.id().clone();

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                let subject = &services.subject;

                let mut events = vec![];
                for signing_algorithm in self
                    .with_fixed_algorithm
                    .map(|signing_algorithm| vec![signing_algorithm])
                    .unwrap_or_else(get_all_enabled_signing_algorithms_supported)
                {
                    let key_id = match signing_algorithm {
                        Algorithm::EdDSA => config().secret_manager.issuer_eddsa_key_id.clone(),
                        Algorithm::ES256 => config().secret_manager.issuer_es256_key_id.clone(),
                        algorithm => return Err(UnsupportedSigningAlgorithmError(algorithm)),
                    };

                    let public_key_jwk = subject
                        .get_public_key(key_id, &signing_algorithm)
                        .await
                        .map_err(|err| MissingKeyError(err.to_string()))?;

                    let verification_method = VerificationMethod::new_from_jwk(
                        did.clone(),
                        public_key_jwk,
                        (did_method == SupportedDidMethod::Key)
                            .then_some(did.method_id())
                            .or(did_method.fragment()),
                    )
                    .map_err(|err| VerificationMethodBuilderError(err.to_string()))?;

                    subject
                        .insert_verification_method_id(
                            StorageKey::new(did_method, signing_algorithm),
                            verification_method.id().clone(),
                        )
                        .await
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    document
                        .insert_method(verification_method, MethodScope::VerificationMethod)
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    events.push(PublicKeyUpdated {
                        document_id: self.document_id.clone(),
                        document: document.clone(),
                    })
                }

                Ok(events)
            }
            UpdateDocumentStatus { status } => Ok(vec![DocumentStatusUpdated {
                document_id: self.document_id.clone(),
                status,
            }]),
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
                if let Some(iota_metadata) = self.iota_metadata.as_ref() {
                    if !iota_metadata.funded {
                        warn!(
                            "Skipping publishing DID Document for DID method `{}` because it is not funded",
                            self.did_method.as_ref().unwrap()
                        );
                        return Ok(vec![]);
                    }
                }

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                // The API endpoint of an IOTA node
                let api_endpoint = did_method
                    .api_endpoint()
                    .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                // Build a new IOTA client to interact with the IOTA ledger.
                let mut iota_client_builder = IotaClientBuilder::default();

                if let Some(iota_node_url) = config().iota_node_url.clone() {
                    iota_client_builder = iota_client_builder.ws_url(iota_node_url);

                    if let Some(iota_node_url_auth) = config().iota_node_username.clone() {
                        if let Some(iota_node_password) = config().iota_node_password.clone() {
                            iota_client_builder =
                                iota_client_builder.basic_auth(iota_node_url_auth, iota_node_password);
                        } else {
                            warn!("No IOTA node URL password configured in the application configuration.");
                        }
                    } else {
                        warn!("No IOTA node URL authentication configured in the application configuration.");
                    }
                } else {
                    warn!("No IOTA node URL configured in the application configuration.");
                }

                let iota_client = iota_client_builder.build(api_endpoint).await.unwrap();

                // Resolve the latest state of the document.
                let document: IotaDocument = self.document.as_ref().ok_or(MissingDocumentError)?.clone().into();

                let stronghold_storage = &services.subject.stronghold_storage;

                // FIXME!
                let key_id = KeyId::new("ed25519-0");

                let public_key_jwk = stronghold_storage.get_ed25519_public_key(&key_id).await.unwrap();

                let storage = &Storage::new(stronghold_storage.clone(), stronghold_storage.clone());

                let signer = StorageSigner::new(storage, key_id, public_key_jwk.clone());

                let read_only_client = IdentityClientReadOnly::new(iota_client.clone()).await.unwrap();
                let identity_client = IdentityClient::new(read_only_client, signer.clone()).await.unwrap();

                let document = match self.status {
                    Status::SignAndValidate => {
                        // Publish the updated Alias Output.
                        let updated_document = identity_client
                            .publish_did_document_update(document.clone(), 50_000_000)
                            .await
                            .unwrap();

                        info!(
                            "Published DID Document: {updated_document}",
                            updated_document = serde_json::to_string_pretty(&updated_document).unwrap()
                        );

                        updated_document
                    }
                    Status::Disabled => {
                        // Deactivate the DID Document
                        identity_client
                            .deactivate_did_output(document.id(), 50_000_000)
                            .await
                            .unwrap();

                        document
                    }
                };

                let iota_metadata = if let Some(iota_metadata) = self.iota_metadata.clone() {
                    Some(IotaMetadata {
                        wallet_address: iota_metadata.wallet_address,
                        funded: true,
                        balance: iota_metadata.balance,
                        created: document.metadata.created.map(|created| created.to_string()),
                        updated: document.metadata.updated.map(|updated| updated.to_string()),
                        ..iota_metadata
                    })
                } else {
                    None
                };

                let wallet_address = IotaAddress::from(&Signer::public_key(&signer).await.unwrap());

                let balances = iota_client
                    .coin_read_api()
                    .get_all_balances(wallet_address)
                    .await
                    .unwrap();

                println!(
                    "Wallet Address: `{}`\nBalances: {}",
                    wallet_address,
                    serde_json::to_string_pretty(&balances).unwrap()
                );

                Ok(vec![DocumentPublished {
                    document_id: self.document_id.clone(),
                    document: CoreDocument::from(document),
                    iota_metadata,
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
            DocumentPublished {
                document_id,
                document,
                iota_metadata,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.iota_metadata = iota_metadata;
            }
        }
    }
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
    async fn test_set_public_key_jwks(
        document_id: String,
        did_method: SupportedDidMethod,
        document: CoreDocument,
        document_with_single_verification_method: CoreDocument,
        document_with_multiple_verification_methods: CoreDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id: document_id.clone(),
                did_method,
                document: document.clone(),
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
                iota_metadata: None,
            }])
            .when(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
            .then_expect_events(vec![
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_single_verification_method,
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id,
                    document: document_with_multiple_verification_methods,
                },
            ])
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
