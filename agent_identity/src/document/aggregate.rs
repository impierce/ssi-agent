use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::{services::IdentityServices, state::get_wallet_address};
use agent_secret_manager::subject::StorageKey;
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::document::CoreDocument;
use identity_iota::iota::Error::DIDUpdateError;
use identity_iota::{
    iota::{IotaClientExt as _, IotaDocument, IotaIdentityClientExt as _},
    verification::{MethodScope, MethodType, VerificationMethod},
};
use iota_sdk::client::api::input_selection::Error::MissingInputWithEd25519Address;
use iota_sdk::client::error::Error::{Block, InputAddressNotFound, InputSelection};
use iota_sdk::types::block::Error::InsufficientStorageDepositAmount;
use iota_sdk::{
    client::Client,
    types::block::{
        address::Bech32Address,
        output::{AliasOutput, AliasOutputBuilder, RentStructure},
    },
};
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ssi_dids::DIDMethod;
use ssi_dids::Source;
use std::{collections::BTreeMap, sync::Arc};
use tracing::{debug, info, warn};

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

                let document = match &did_method {
                    SupportedDidMethod::Iota | SupportedDidMethod::IotaSmr => {
                        // The API endpoint of an IOTA node, e.g. Hornet.
                        let api_endpoint = did_method
                            .api_endpoint()
                            .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                        // Retrieve the network name associated with the DID method.
                        let network_name = did_method.network_name().ok_or(MissingNetworkNameError(did_method))?;

                        // Build a new IOTA client to interact with the IOTA ledger.
                        let iota_client: Client = Client::builder()
                            .with_node(api_endpoint)
                            .map_err(|_| InvalidNodeEndpointError(api_endpoint.to_string()))?
                            .finish()
                            .await
                            .map_err(|err| IotaClientBuilderError(err.to_string()))?;

                        // Retrieve the current wallet address from the Stronghold storage.
                        let wallet_address: Bech32Address =
                            get_wallet_address(&iota_client, stronghold_storage.as_secret_manager())
                                .await
                                .map_err(|err| WalletAddressError(err.to_string()))?;

                        info!("Current {network_name} Address: `{wallet_address}`");

                        // Check if a DID Document already exists in the aggregate.
                        // If so, attempt to publish it to validate that the current wallet address is in control of it.
                        let document = if let Some(document) = self.document.clone().map(IotaDocument::from) {
                            let controller = document.id().clone();
                            info!("Found an existing controller for DID method `{did_method}`: `{controller}`");

                            // Create a new DID Document from scratch.
                            let document = IotaDocument::new_with_id(controller.clone());

                            let rent_structure: RentStructure =
                                iota_client.get_rent_structure().await.map_err(IotaClientError)?;

                            // Update the DID Document output with the latest state.
                            let alias_output: AliasOutput =
                                iota_client.update_did_output(document).await.map_err(IotaClientError)?;

                            let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
                                .with_minimum_storage_deposit(rent_structure)
                                .finish()
                                .map_err(|err| AliasOutputBuilderError(err.to_string()))?;

                            // Publish the updated Alias Output and get the published DID document.
                            let publish_result = iota_client
                                .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                                .await
                                .map(CoreDocument::from);

                            match publish_result {
                                // The current wallet address controls the existing DID Document.
                                Ok(document) => Some(document),
                                Err(test_publish_error) => match test_publish_error {
                                    DIDUpdateError(_, Some(ref error)) => {
                                        // This specific error signifies that the current wallet address is NOT in
                                        // control of the DID Document found in the Aggregate.
                                        if let InputAddressNotFound { address, .. } = &**error {
                                            warn!(
                                                "The current `{did_method}` DID `{controller}` is controlled by wallet address `{address}`, \
                                                but the wallet address connected to the current Stronghold file on the {network_name} network is `{wallet_address}`."
                                                );
                                            // We don't return an error here. Instead we assign `None` to `document` so
                                            // that later on a new DID Document will be created using the current
                                            // wallet address.
                                            None
                                        } else if let Block(InsufficientStorageDepositAmount { amount, required }) =
                                            &**error
                                        {
                                            warn!(
                                                "The current `{did_method}` DID `{controller}` has insufficient storage deposit amount: `{amount}`, \
                                                required: `{required}`."
                                                );
                                            return Err(InsufficientDepositError(
                                                network_name.to_string(),
                                                wallet_address.to_string(),
                                            ));
                                        } else {
                                            return Err(IotaClientError(test_publish_error));
                                        }
                                    }
                                    other_test_publish_error => return Err(IotaClientError(other_test_publish_error)),
                                },
                            }
                        } else {
                            None
                        };

                        if let Some(document) = document {
                            // Return the DID Document that was already stored in the Aggregate now we validated that
                            // the current Stronghold storage is in control of it.
                            document
                        } else {
                            // If there was no DID Document stored in the Aggregate yet, or the current Stronghold
                            // storage is not in control of it, then we create a completely new controller and DID Document.
                            info!("Creating a new controller for DID method `{did_method}`");

                            // Create a new 'blank' DID Document.
                            let document =
                                IotaDocument::new(&iota_client.network_name().await.map_err(IotaClientError)?);

                            // Construct an Alias Output containing the DID document, with the wallet address
                            // set as both the state controller and governor.
                            let alias_output: AliasOutput = iota_client
                                .new_did_output(*wallet_address, document, None)
                                .await
                                .map_err(IotaClientError)?;

                            // Publish the Alias Output and get the published DID document.
                            let publish_result = iota_client
                                .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                                .await
                                .map(CoreDocument::from);

                            match publish_result {
                                // Creating and publishing the new DID Document was successful.
                                Ok(document) => document,
                                // This error indicates that the Wallet Address does not have sufficient funds and
                                // therefore we need to throw an explixit `InsufficientDepositError` error message.
                                Err(DIDUpdateError(_, Some(error)))
                                    if matches!(*error, InputSelection(MissingInputWithEd25519Address)) =>
                                {
                                    return Err(InsufficientDepositError(
                                        network_name.to_string(),
                                        wallet_address.to_string(),
                                    ));
                                }
                                Err(other_error) => return Err(IotaClientError(other_error)),
                            }
                        }
                    }
                    SupportedDidMethod::Web => {
                        let origin = config().url.clone().expect("TODO: should never be None").origin();

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

                Ok(vec![DocumentCreated {
                    document_id,
                    did_method,
                    status,
                    document,
                    with_fixed_algorithm,
                }])
            }
            UpdatePublicKeys {
                // TODO: decide whether the public keys should be supplied through the command or not.
                public_key_jwks: _,
            } => {
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
                    .insert_service(service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document_id, document }])
            }
            PublishDocument => {
                // The API endpoint of an IOTA node, e.g. Hornet.
                let api_endpoint = self
                    .did_method
                    .as_ref()
                    .and_then(SupportedDidMethod::api_endpoint)
                    .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                // Create a new client to interact with the IOTA ledger.
                let iota_client: Client = Client::builder()
                    .with_node(api_endpoint)
                    .map_err(|_| InvalidNodeEndpointError(api_endpoint.to_string()))?
                    .finish()
                    .await
                    .map_err(|err| IotaClientBuilderError(err.to_string()))?;

                // Resolve the latest state of the document.
                let document: IotaDocument = self.document.as_ref().ok_or(MissingDocumentError)?.clone().into();

                let alias_output = match self.status {
                    Status::SignAndValidate => {
                        // Resolve the latest output and update it with the given document.
                        let alias_output: AliasOutput =
                            iota_client.update_did_output(document).await.map_err(IotaClientError)?;

                        alias_output
                    }
                    Status::Disabled => {
                        // Deactivate the DID by publishing an empty document.
                        // This process can be reversed since the Alias Output is not destroyed.
                        // Deactivation may only be performed by the state controller of the Alias Output.
                        let deactivated_output: AliasOutput = iota_client
                            .deactivate_did_output(document.id())
                            .await
                            .map_err(IotaClientError)?;

                        deactivated_output
                    }
                };

                // Because the size of the DID document increased, we have to increase the allocated storage deposit.
                // This increases the deposit amount to the new minimum.
                let rent_structure: RentStructure = iota_client.get_rent_structure().await.map_err(IotaClientError)?;
                let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
                    .with_minimum_storage_deposit(rent_structure)
                    .finish()
                    .map_err(|err| AliasOutputBuilderError(err.to_string()))?;

                let stronghold_storage = &services.subject.stronghold_storage;

                // Publish the updated Alias Output.
                let updated_document = iota_client
                    .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                    .await
                    .map(CoreDocument::from)
                    .map_err(IotaClientError)?;

                Ok(vec![DocumentPublished {
                    document_id: self.document_id.clone(),
                    document: updated_document,
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
            } => {
                self.document_id = document_id;
                self.did_method.replace(did_method);
                self.status = status;
                self.document.replace(document);
                self.with_fixed_algorithm = with_fixed_algorithm;
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
            DocumentPublished { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
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
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_set_public_key_jwks(
        document_id: String,
        did_method: SupportedDidMethod,
        document: CoreDocument,
        document_with_verification_method: CoreDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated {
                document_id: document_id.clone(),
                did_method,
                document,
                status: Status::SignAndValidate,
                with_fixed_algorithm: None,
            }])
            .when(DocumentCommand::UpdatePublicKeys {
                public_key_jwks: vec![],
            })
            .then_expect_events(vec![DocumentEvent::PublicKeyUpdated {
                document_id,
                document: document_with_verification_method,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_service(
        document_id: String,
        did_method: SupportedDidMethod,
        document: CoreDocument,
        domain_linkage_service: Service,
        document_with_verification_method: CoreDocument,
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
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_verification_method,
                },
            ])
            .when(DocumentCommand::AddService {
                service: domain_linkage_service,
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
        document_with_verification_method: CoreDocument,
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
                },
                DocumentEvent::PublicKeyUpdated {
                    document_id: document_id.clone(),
                    document: document_with_verification_method.clone(),
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
    use agent_shared::config::config;
    use agent_shared::config::SupportedDidMethod;
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
    pub fn verification_method() -> VerificationMethod {
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
    pub fn document_with_verification_method(
        mut document: CoreDocument,
        verification_method: VerificationMethod,
    ) -> CoreDocument {
        document
            .insert_method(verification_method, MethodScope::VerificationMethod)
            .unwrap();

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
                    "origins": [config().url],
                }))
                .unwrap(),
            )
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn document_with_domain_linkage_service(
        mut document_with_verification_method: CoreDocument,
        domain_linkage_service: Service,
    ) -> CoreDocument {
        document_with_verification_method
            .insert_service(domain_linkage_service)
            .unwrap();

        document_with_verification_method
    }
}
