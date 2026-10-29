use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::convert::convert;
use crate::document::openapi::algorithm;
use crate::services::IdentityServices;
use agent_secret_manager::subject::StorageKey;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use agent_shared::config::{config_mut, SupportedDidMethod};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_document::document::CoreDocument;
use identity_iota::iota::rebased::client::{get_object_id_from_did, IdentityClient, PublishDidDocument};
use identity_iota::iota::rebased::migration::{ControllerToken, Identity, OnChainIdentity};
use identity_iota::iota::IotaDocument;
use identity_iota::iota::{rebased, IotaDID};
use identity_iota::storage::{Storage, StorageSigner};
use identity_kit::did::{
    Context, Did, DidDocument, DidUrl, MethodType, VerificationMaterial, VerificationMethod, VerificationRelationship,
};
use identity_kit::jose::Jwk;
use identity_kit::methods::{jwk::DidJwk, key::DidKey};
use identity_storage::{JwkStorage, KeyIdStorage};
use iota_sdk::types::base_types::IotaAddress;
use iota_sdk::{IotaClient, IotaClientBuilder};
use jsonwebtoken::Algorithm;
use product_common::gas_station::GasStationOptions;
use product_common::network_name::NetworkName;
use product_common::transaction::TransactionBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};
use url::Url;

// TODO: look into a more appropriate value for the minimum gas budget. This current value of `50_000_000` as adopted
// from examples from the IOTA identity library.
/// Minimum gas budget for publishing a DID Document on the IOTA ledger.
const MIN_GAS_BUDGET: u64 = 50_000_000;

/// Metadata for IOTA-based DID Documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, utoipa::ToSchema)]
pub struct IotaMetadata {
    #[schema(value_type = String, example = "0x1234567890")]
    pub wallet_address: IotaAddress,
    pub is_funded: bool,
    pub balance: u64,
    pub is_published: bool,
    pub is_deactivated: bool,
    pub explorer_url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub enum Status {
    SignAndValidate,
    // TODO: Make a distinction between enabling both signing AND validation and just validation.
    // ValidateOnly,
    #[default]
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Document {
    #[serde(rename = "id")]
    pub document_id: String,
    pub document: Option<DidDocument>,
    pub did_method: Option<SupportedDidMethod>,
    // Applicable only for DID documents whose methods mandate a fixed verification algorithm,
    // such as `did:key` and `did:jwk`.
    #[schema(schema_with = algorithm)]
    pub with_fixed_algorithm: Option<Algorithm>,
    // Applicable only for DID methods that are based on the IOTA ledger.
    pub iota_metadata: Option<IotaMetadata>,
    #[schema(inline)]
    pub status: Status,
    /// The verification methods of keys that aren't configured issuer keys, such as KMS keys, in the order they were
    /// added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schema(value_type = Vec<String>)]
    pub added_verification_methods: Vec<DidUrl>,
}

impl Aggregate for Document {
    type Command = DocumentCommand;
    type Event = DocumentEvent;
    type Error = DocumentError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "document";

    // TODO: Most of how these commands are handled is not Domain logic, but rather Application logic, so it should be moved
    // to the Application layer. The Aggregate should only handle the Domain logic, such as creating a new Document, updating public keys, etc.
    // The Application layer should handle the specifics of how to create a Document based on the DID method, how to publish it, etc.
    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use DocumentCommand::*;
        use DocumentError::*;
        use DocumentEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
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

                        // Create an `IdentityClient` instance.
                        // This client is used to interact with the IOTA identity ledger.
                        // It is used to publish the DID Document and to resolve it later.
                        let identity_client = IdentityClient::from_iota_client(iota_client.clone(), None)
                            .await
                            .map_err(|err| GenericError(err.to_string()))?
                            .with_signer(signer)
                            .await
                            .map_err(|err| GenericError(err.to_string()))?;

                        // Retrieve the wallet address from the identity client.
                        let wallet_address = identity_client.address();

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

                        let document = self
                            .document
                            .as_ref()
                            .map(to_iota_document)
                            .transpose()?
                            .unwrap_or_else(|| {
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

                        from_iota_document(document)?
                    }
                    SupportedDidMethod::Web => {
                        let controller = super::web::did_web(&services.public_url)?;

                        // Patch the generated DID document since it's not according to spec.
                        DidDocument::builder(controller)
                            .context(get_context(&MethodType::JSON_WEB_KEY_2020))
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
                            .get_public_key(key_id.clone())
                            .await
                            .map_err(|err| MissingKeyError(err.to_string()))?;

                        // Generate a DID from the public key.
                        let controller = match did_method {
                            SupportedDidMethod::Jwk => DidJwk::did(&public_key_jwk).ok(),
                            SupportedDidMethod::Key => DidKey::did(&public_key_jwk).ok(),
                            _ => None,
                        }
                        .ok_or(GenerateDidError(key_id))?;

                        DidDocument::new(controller)
                    }
                };

                let status = Status::SignAndValidate;

                let iota_metadata = (did_method == SupportedDidMethod::Iota
                    || did_method == SupportedDidMethod::IotaDev
                    || did_method == SupportedDidMethod::IotaTest)
                    .then_some(iota_metadata);

                Ok::<Vec<Self::Event>, Self::Error>(vec![DocumentCreated {
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
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let did = document.id.clone();

                let did_method = self.did_method.ok_or(MissingDidMethodError)?;

                let subject = &services.subject;

                // Remove the verification methods of the configured issuer keys from the document.
                let method_ids = document
                    .methods(None)
                    .map(|method| method.id.clone())
                    .filter(|method_id| !self.added_verification_methods.contains(method_id))
                    .collect::<Vec<_>>();
                for method_id in method_ids {
                    document.remove_verification_method(&method_id);
                }

                // Insert the new verification methods based on the signing algorithms.
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
                        .get_public_key(key_id)
                        .await
                        .map_err(|err| MissingKeyError(err.to_string()))?;

                    let verification_method = verification_method(&did, did_method, &public_key_jwk)?;

                    subject
                        .insert_verification_method_id(
                            StorageKey::new(did_method, signing_algorithm),
                            verification_method.id.clone(),
                        )
                        .await
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    // Attach all possible relationships to the verification method.
                    document
                        .insert_verification_method(verification_method, VerificationRelationship::ALL.iter().copied()) // TODO: adjust KID insertion elsewhere
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    events.push(PublicKeyUpdated {
                        document_id: self.document_id.clone(),
                        document: document.clone(),
                    })
                }

                Ok(events)
            }
            AddPublicKey { public_key_jwk } => {
                let did_method = self.did_method.ok_or(MissingDidMethodError)?;
                if did_method != SupportedDidMethod::Web {
                    return Err(AddPublicKeyUnsupportedError(did_method));
                }
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                if public_key_jwk.alg().is_none() {
                    return Err(InvalidPublicKeyError("it names no algorithm".into()));
                }
                // The thumbprint identifies the key in the document, as for the configured issuer keys, rather than
                // whatever `kid` the key's store gave it.
                let thumbprint = public_key_jwk
                    .thumbprint_sha256()
                    .map_err(|err| InvalidPublicKeyError(err.to_string()))?;
                let public_key_jwk = public_key_jwk.public().with_kid(thumbprint);
                let verification_method = verification_method(&document.id, did_method, &public_key_jwk)?;
                let verification_method_id = verification_method.id.clone();

                if document.methods(None).any(|method| method.id == verification_method_id) {
                    return Ok(());
                }
                document
                    .insert_verification_method(verification_method, VerificationRelationship::ALL.iter().copied())
                    .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                Ok(vec![PublicKeyAdded {
                    document_id: self.document_id.clone(),
                    document,
                    verification_method_id,
                }])
            }
            OverwritePreviousDidWeb {
                previous_did,
                public_url,
            } => {
                if self.did_method != Some(SupportedDidMethod::Web) {
                    return Err(InvalidDidError(
                        "Only did:web supports overwriting the deployment identity".into(),
                    ));
                }
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let previous = document.id.clone();
                if previous != previous_did {
                    return Err(InvalidDidError(format!(
                        "Overwrite authorization for {previous_did} does not match the current DID {previous}"
                    )));
                }
                let replacement = super::web::did_web(&public_url)?;
                document.map_dids(|did| if did == previous { replacement.clone() } else { did });
                Ok(vec![DocumentDidWebOverwritten {
                    document_id: self.document_id.clone(),
                    previous_did: previous,
                    document,
                }])
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

                // Set the service ID.
                service.id = document
                    .id
                    .join(&format!("#{service_id}"))
                    .map_err(|_| InvalidDidError(service_id.to_string()))?;

                // Overwrite the service if it already exists.
                document.remove_service(&service.id);
                document
                    .insert_service(*service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document_id, document }])
            }
            RemoveService { service_id } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let id = document
                    .id
                    .join(&format!("#{service_id}"))
                    .map_err(|err| InvalidDidError(err.to_string()))?;
                if document.remove_service(&id).is_none() {
                    return Ok(());
                }
                Ok(vec![ServiceRemoved {
                    document_id: self.document_id.clone(),
                    document,
                }])
            }
            PublishDocument => {
                let mut document = to_iota_document(self.document.as_ref().ok_or(MissingDocumentError)?)?;

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

                // Create an `IdentityClient` instance.
                // This client is used to interact with the IOTA identity ledger.
                // It is used to publish the DID Document and to resolve it later.
                let identity_client = IdentityClient::from_iota_client(iota_client.clone(), None)
                    .await
                    .map_err(|err| GenericError(err.to_string()))?
                    .with_signer(signer)
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
                        return Ok(());
                    }
                }

                // Retrieve the wallet address from the identity client.
                let wallet_address = identity_client.address();

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
                        .map(to_iota_document)
                        .transpose()?
                        .map(|document| document.id().to_owned())
                        .unwrap_or_else(|| IotaDID::placeholder(&network_name));

                    let document = from_iota_document(IotaDocument::new_with_id(did))?;

                    Ok(vec![DocumentDeleted {
                        document_id: self.document_id.clone(),
                        document,
                    }])
                } else {
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
                        document: from_iota_document(document)?,
                        iota_metadata: Some(iota_metadata),
                    }])
                }
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
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
            PublicKeyUpdated { document_id, document }
            | DocumentDidWebOverwritten {
                document_id, document, ..
            } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            PublicKeyAdded {
                document_id,
                document,
                verification_method_id,
            } => {
                self.document_id = document_id;
                self.document.replace(document);
                self.added_verification_methods.push(verification_method_id);
            }
            DocumentStatusUpdated { document_id, status } => {
                self.document_id = document_id;
                self.status = status;
            }
            ServiceAdded { document_id, document } | ServiceRemoved { document_id, document } => {
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
            identity_client.address(),
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

fn to_iota_document(document: &DidDocument) -> Result<IotaDocument, DocumentError> {
    convert::<CoreDocument>(document)
        .map(IotaDocument::from)
        .map_err(|err| DocumentError::GenericError(err.to_string()))
}

fn from_iota_document(document: IotaDocument) -> Result<DidDocument, DocumentError> {
    convert(&CoreDocument::from(document)).map_err(|err| DocumentError::GenericError(err.to_string()))
}

/// A `JsonWebKey2020` verification method for `public_key_jwk`. Its fragment is the method-specific identifier for
/// `did:key`, the fragment `did_method` mandates, if any, or else the key's `kid`.
fn verification_method(
    did: &Did,
    did_method: SupportedDidMethod,
    public_key_jwk: &Jwk,
) -> Result<VerificationMethod, DocumentError> {
    let fragment = (did_method == SupportedDidMethod::Key)
        .then_some(did.method_specific_id())
        .or(did_method.fragment())
        .or(public_key_jwk.kid())
        .ok_or_else(|| {
            DocumentError::VerificationMethodBuilderError("an explicit fragment or JWK kid is required".into())
        })?;
    let id = did
        .join(&format!("#{fragment}"))
        .map_err(|err| DocumentError::VerificationMethodBuilderError(err.to_string()))?;

    Ok(VerificationMethod::new(
        id,
        MethodType::JSON_WEB_KEY_2020,
        did.clone(),
        VerificationMaterial::jwk(public_key_jwk),
    ))
}

// TODO: Can we remove this? It does not seem to be required: https://w3c-ccg.github.io/did-method-web/#key-material-and-document-handling
pub fn get_context(method_type: &MethodType) -> Context {
    match method_type.as_str() {
        "Ed25519VerificationKey2018" => {
            Context::uris([Context::DID_V1, "https://w3id.org/security/suites/ed25519-2018/v1"])
        }
        "JsonWebKey2020" => Context::uris([Context::DID_V1, "https://w3id.org/security/suites/jws-2020/v1"]),
        _ => unimplemented!("Unsupported method type"),
    }
}

#[cfg(test)]
pub mod document_tests {
    use crate::state::LINKED_DOMAINS_SERVICE_ID;

    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use identity_kit::did::Service;
    use rstest::rstest;

    type DocumentTestFramework = TestFramework<Document>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_document(document_id: String, did_method: SupportedDidMethod, document: DidDocument) {
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
    #[case::did_key_eddsa(
        SupportedDidMethod::Key,
        Algorithm::EdDSA,
        "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt"
    )]
    #[case::did_key_es256(
        SupportedDidMethod::Key,
        Algorithm::ES256,
        "did:key:zDnaeRwT4g6AZCHzxvNL7DLjqTaT88am4XR6TUGrKr6DXj6Tz"
    )]
    #[case::did_jwk_eddsa(
        SupportedDidMethod::Jwk,
        Algorithm::EdDSA,
        "did:jwk:eyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ"
    )]
    #[case::did_jwk_es256(
        SupportedDidMethod::Jwk,
        Algorithm::ES256,
        "did:jwk:eyJhbGciOiJFUzI1NiIsImNydiI6IlAtMjU2Iiwia2lkIjoib09ZMmRNVlU3R0s1YWwxcTdFQXh1b1lsb3hNUWx2NVpOWk9hdGlVWFFIZyIsImt0eSI6IkVDIiwieCI6IkZtazEzZ08yU0dMYnVYZUwyNHFKUEhDTm5jbkk2bEJ1NlpRTDJFVlp2NEUiLCJ5IjoiZnoyS3ZNaHVmelVwTWVMOS1LMnJlOWZ3QTNtemcxYnBmYmNlSVFTdWloWSJ9"
    )]
    #[serial_test::serial]
    async fn test_create_document_derives_the_did_from_the_issuer_key(
        document_id: String,
        #[case] did_method: SupportedDidMethod,
        #[case] algorithm: Algorithm,
        #[case] expected_did: &str,
    ) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id,
                did_method,
                with_fixed_algorithm: Some(algorithm),
            })
            .inspect_result()
            .unwrap();

        let [DocumentEvent::DocumentCreated { document, .. }] = events.as_slice() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(document.id.as_str(), expected_did);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_set_public_key_jwks(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        document_with_single_verification_method: DidDocument,
        document_with_multiple_verification_methods: DidDocument,
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

    /// An EdDSA key as a KMS returns it, with the KMS's own `kid`.
    fn kms_public_key() -> Jwk {
        serde_json::from_value(serde_json::json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
            "alg": "EdDSA",
            "kid": "arn:aws:kms:eu-central-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab"
        }))
        .unwrap()
    }

    fn kms_verification_method() -> VerificationMethod {
        VerificationMethod::new(
            "did:web:my-domain.example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
                .parse()
                .unwrap(),
            MethodType::JSON_WEB_KEY_2020,
            "did:web:my-domain.example.org".parse().unwrap(),
            VerificationMaterial::jwk(
                &serde_json::from_value::<Jwk>(serde_json::json!({
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo",
                    "alg": "EdDSA",
                    "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k"
                }))
                .unwrap(),
            ),
        )
    }

    fn with_kms_verification_method(mut document: DidDocument) -> DidDocument {
        document
            .insert_verification_method(kms_verification_method(), VerificationRelationship::ALL.iter().copied())
            .unwrap();

        document
    }

    fn document_created(document_id: &str, did_method: SupportedDidMethod, document: DidDocument) -> DocumentEvent {
        DocumentEvent::DocumentCreated {
            document_id: document_id.to_string(),
            did_method,
            document,
            status: Status::SignAndValidate,
            with_fixed_algorithm: None,
            iota_metadata: None,
        }
    }

    fn kms_public_key_added(document_id: &str, document: DidDocument) -> DocumentEvent {
        DocumentEvent::PublicKeyAdded {
            document_id: document_id.to_string(),
            document: with_kms_verification_method(document),
            verification_method_id: kms_verification_method().id,
        }
    }

    #[rstest]
    #[serial_test::serial]
    async fn adds_a_public_key_named_by_its_thumbprint(document_id: String, document: DidDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![document_created(
                &document_id,
                SupportedDidMethod::Web,
                document.clone(),
            )])
            .when(DocumentCommand::AddPublicKey {
                public_key_jwk: Box::new(kms_public_key()),
            })
            .then_expect_events(vec![kms_public_key_added(&document_id, document)])
    }

    #[rstest]
    #[serial_test::serial]
    async fn adding_a_public_key_again_adds_nothing(document_id: String, document: DidDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                document_created(&document_id, SupportedDidMethod::Web, document.clone()),
                kms_public_key_added(&document_id, document),
            ])
            .when(DocumentCommand::AddPublicKey {
                public_key_jwk: Box::new(kms_public_key()),
            })
            .then_expect_events(vec![])
    }

    #[rstest]
    #[serial_test::serial]
    async fn only_did_web_documents_add_public_keys(document_id: String, document: DidDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![document_created(&document_id, SupportedDidMethod::Jwk, document)])
            .when(DocumentCommand::AddPublicKey {
                public_key_jwk: Box::new(kms_public_key()),
            })
            .then_expect_error_message("Only `did:web` documents can add public keys, not `did:jwk` documents")
    }

    #[rstest]
    #[serial_test::serial]
    async fn a_public_key_to_add_must_name_its_algorithm(document_id: String, document: DidDocument) {
        let public_key_jwk: Jwk = serde_json::from_value(serde_json::json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        }))
        .unwrap();

        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![document_created(&document_id, SupportedDidMethod::Web, document)])
            .when(DocumentCommand::AddPublicKey {
                public_key_jwk: Box::new(public_key_jwk),
            })
            .then_expect_error_message("The public key to add is invalid: it names no algorithm")
    }

    #[rstest]
    #[serial_test::serial]
    async fn updating_the_public_keys_keeps_the_added_ones(
        document_id: String,
        document: DidDocument,
        both_verification_methods: Vec<VerificationMethod>,
    ) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given(vec![
                document_created(&document_id, SupportedDidMethod::Web, document.clone()),
                kms_public_key_added(&document_id, document),
            ])
            .when(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
            .inspect_result()
            .unwrap();

        let Some(DocumentEvent::PublicKeyUpdated { document, .. }) = events.last() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(
            document.methods(None).cloned().collect::<Vec<_>>(),
            [vec![kms_verification_method()], both_verification_methods].concat()
        );
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_service(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        linked_domains_service: Service,
        document_with_multiple_verification_methods: DidDocument,
        document_with_linked_domains_service: DidDocument,
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
                service: Box::new(linked_domains_service),
                service_id: LINKED_DOMAINS_SERVICE_ID.to_string(),
            })
            .then_expect_events(vec![DocumentEvent::ServiceAdded {
                document_id: document_id.clone(),
                document: document_with_linked_domains_service,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_set_status(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
        document_with_multiple_verification_methods: DidDocument,
        document_with_linked_domains_service: DidDocument,
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
                    document: document_with_linked_domains_service,
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

    #[rstest]
    #[serial_test::serial]
    async fn did_web_documents_keep_their_context(document_id: String) {
        let events = DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument {
                document_id,
                did_method: SupportedDidMethod::Web,
                with_fixed_algorithm: None,
            })
            .inspect_result()
            .unwrap();

        let [DocumentEvent::DocumentCreated { document, .. }] = events.as_slice() else {
            panic!("unexpected events: {events:?}");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap()["@context"],
            serde_json::json!([
                "https://www.w3.org/ns/did/v1",
                "https://w3id.org/security/suites/jws-2020/v1"
            ])
        );
    }

    #[rstest]
    #[serial_test::serial]
    fn documents_round_trip_through_core_documents(document_with_linked_domains_service: DidDocument) {
        let core_document: CoreDocument = convert(&document_with_linked_domains_service).unwrap();

        assert_eq!(
            convert::<DidDocument>(&core_document).unwrap(),
            document_with_linked_domains_service
        );
    }

    #[rstest]
    #[serial_test::serial]
    async fn overwrite_rejects_an_authorization_for_another_did(
        document_id: String,
        did_method: SupportedDidMethod,
        document: DidDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id,
                did_method,
                document,
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
                iota_metadata: None,
            }])
            .when(DocumentCommand::OverwritePreviousDidWeb {
                previous_did: "did:web:other.example".parse().unwrap(),
                public_url: "https://new.example".parse().unwrap(),
            })
            .then_expect_error_message(
                "Invalid DID: Overwrite authorization for did:web:other.example does not match the current DID \
                 did:web:my-domain.example.org",
            )
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::get_context;
    use crate::state::LINKED_DOMAINS_SERVICE_ID;
    use agent_shared::config::{config, SupportedDidMethod};
    use identity_kit::did::{
        Did, DidDocument, MethodType, Service, VerificationMaterial, VerificationMethod, VerificationRelationship,
    };
    use identity_kit::jose::Jwk;
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

    fn controller() -> Did {
        "did:web:my-domain.example.org".parse().unwrap()
    }

    #[fixture]
    pub fn document() -> DidDocument {
        DidDocument::builder(controller())
            .context(get_context(&MethodType::JSON_WEB_KEY_2020))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn es256_verification_method() -> VerificationMethod {
        VerificationMethod::new(
            "did:web:my-domain.example.org#oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg"
                .parse()
                .unwrap(),
            MethodType::JSON_WEB_KEY_2020,
            controller(),
            VerificationMaterial::jwk(
                &serde_json::from_value::<Jwk>(json!({
                    "kty": "EC",
                    "alg": "ES256",
                    "kid": "oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg",
                    "crv": "P-256",
                    "x": "Fmk13gO2SGLbuXeL24qJPHCNncnI6lBu6ZQL2EVZv4E",
                    "y": "fz2KvMhufzUpMeL9-K2re9fwA3mzg1bpfbceIQSuihY"
                }))
                .unwrap(),
            ),
        )
    }

    #[fixture]
    pub fn eddsa_verification_method() -> VerificationMethod {
        VerificationMethod::new(
            "did:web:my-domain.example.org#bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0"
                .parse()
                .unwrap(),
            MethodType::JSON_WEB_KEY_2020,
            controller(),
            VerificationMaterial::jwk(
                &serde_json::from_value::<Jwk>(json!({
                    "kty": "OKP",
                    "alg": "EdDSA",
                    "kid": "bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0",
                    "crv": "Ed25519",
                    "x": "GlnK9ePs802XxAglROQzoGurm9Qpv0IFPEbdMCILN_U"
                }))
                .unwrap(),
            ),
        )
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
        mut document: DidDocument,
        es256_verification_method: VerificationMethod,
    ) -> DidDocument {
        // Attach all possible relationships to the verification method.
        document
            .insert_verification_method(es256_verification_method, VerificationRelationship::ALL.iter().copied())
            .unwrap();

        document
    }

    #[fixture]
    pub fn document_with_multiple_verification_methods(
        mut document: DidDocument,
        both_verification_methods: Vec<VerificationMethod>,
    ) -> DidDocument {
        for verification_method in both_verification_methods {
            document
                .insert_verification_method(verification_method, VerificationRelationship::ALL.iter().copied())
                .unwrap();
        }

        document
    }

    #[fixture]
    pub fn linked_domains_service() -> Service {
        serde_json::from_value(json!({
            "id": format!("did:web:my-domain.example.org#{LINKED_DOMAINS_SERVICE_ID}"),
            "type": "LinkedDomains",
            // An origin as UniCore writes it: a URL with an empty path.
            "serviceEndpoint": {
                "origins": [format!("{}/", config().public_url.origin().ascii_serialization())],
            },
        }))
        .unwrap()
    }

    #[fixture]
    pub fn document_with_linked_domains_service(
        mut document_with_multiple_verification_methods: DidDocument,
        linked_domains_service: Service,
    ) -> DidDocument {
        document_with_multiple_verification_methods
            .insert_service(linked_domains_service)
            .unwrap();

        document_with_multiple_verification_methods
    }
}
