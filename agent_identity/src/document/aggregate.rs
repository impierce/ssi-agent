use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::{services::IdentityServices, state::get_address};
use agent_secret_manager::{
    subject::{Algorithms, DocumentData},
    ED25519_KEY_ID, ES256_KEY_ID, STRONGHOLD_PATH,
};
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported, SecretManagerConfig};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::document::CoreDocument;
use identity_iota::{
    iota::{IotaClientExt as _, IotaDID, IotaDocument, IotaIdentityClientExt as _, NetworkName},
    storage::KeyId,
    verification::{jwk::Jwk, MethodData, MethodScope, MethodType, VerificationMethod},
};
use iota_sdk::{
    client::{
        secret::{stronghold::StrongholdSecretManager, SecretManager},
        Client, Password,
    },
    types::block::{
        address::{Address, Bech32Address},
        output::{AliasOutput, AliasOutputBuilder, RentStructure},
    },
};
use jsonwebtoken::Algorithm;
use reqwest::{header::HeaderMap, Method};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tracing::info;

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
            CreateDocument { did_method } => {
                let document_id = did_method.to_string();

                let stronghold_storage = &services.subject.stronghold_storage;
                let mut did_methods = services.subject.did_methods.lock().await;

                let document = match &did_method {
                    SupportedDidMethod::Iota | SupportedDidMethod::IotaSmr | SupportedDidMethod::IotaRms => {
                        // The API endpoint of an IOTA node, e.g. Hornet.
                        let api_endpoint = did_method
                            .api_endpoint()
                            .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                        // Create a new client to interact with the IOTA ledger.
                        let iota_client: Client = Client::builder()
                            .with_node(api_endpoint)
                            .map_err(|_| InvalidNodeEndpointError(api_endpoint.to_string()))?
                            .finish()
                            .await
                            .map_err(|err| IotaClientBuilderError(err.to_string()))?;

                        let address: Bech32Address = get_address(&iota_client, stronghold_storage.as_secret_manager())
                            .await
                            .map_err(|err| SecretManagerInitializationError(err.to_string()))?;

                        let ledger_sponsoring_service = config().ledger_sponsoring_service.clone().expect(
                            "Ledger sponsoring service not configured. Please configure the `ledger_sponsoring_service` in the config file.",
                        );
                        let access_key = ledger_sponsoring_service.access_key;
                        let url = ledger_sponsoring_service.url;

                        let client = reqwest::Client::new();

                        let json = json!({
                            "RequestSponsoring": {
                                "access_key": access_key,
                                "amount": 200000,
                                "address": address
                            }
                        });

                        // TODO: remove this once the ledger sponsoring service does not require authorization anymore.
                        let authorization = ledger_sponsoring_service.authorization;
                        let mut headers = HeaderMap::new();
                        headers.insert("Authorization", authorization.parse().expect("Invalid authorization"));

                        info!("Requesting funds for address: `{}`", address);

                        let _ = client
                            .request(Method::POST, url)
                            .headers(headers)
                            .json(&json)
                            .send()
                            .await;

                        // TODO: poll the ledger until the address is sponsored.
                        std::thread::sleep(std::time::Duration::from_secs(18));

                        let address: Address = *address;

                        let network_name: NetworkName = iota_client.network_name().await.map_err(IotaClientError)?;
                        let document: IotaDocument = IotaDocument::new(&network_name);

                        // Construct an Alias Output containing the DID document, with the wallet address
                        // set as both the state controller and governor.
                        let alias_output: AliasOutput = iota_client
                            .new_did_output(address, document, None)
                            .await
                            .map_err(IotaClientError)?;

                        // Publish the Alias Output and get the published DID document.
                        let document: IotaDocument = iota_client
                            .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                            .await
                            .map_err(IotaClientError)?;

                        CoreDocument::from(document)
                    }
                    SupportedDidMethod::Web => {
                        let origin = config().url.origin();

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
                            .map_err(|err| ProduceDocumentError(err.to_string()))?
                    }
                    _is_not_updateable => {
                        return Err(MethodNotUpdateableError(did_method.to_string()));
                    }
                };

                did_methods.insert(
                    &did_method,
                    Algorithms {
                        es256: Some(DocumentData {
                            did: document.id().to_string(),
                            verification_method_id: None,
                        }),
                        eddsa: Some(DocumentData {
                            did: document.id().to_string(),
                            verification_method_id: None,
                        }),
                    },
                );

                let status = Status::SignAndValidate;

                Ok(vec![DocumentCreated {
                    document_id,
                    status,
                    document,
                }])
            }
            SetPublicKeyJwks {
                did_method,
                // TODO: decide whether the public keys should be suplied through the command or not.
                public_key_jwks: _,
            } => {
                let mut document = self
                    .document
                    .clone()
                    .ok_or_else(|| ProduceDocumentError(did_method.to_string()))?;
                let mut did_methods = services.subject.did_methods.lock().await;

                let did = document.id().clone();

                let stronghold_storage = &services.subject.stronghold_storage;

                let mut public_key_jwks = vec![];

                for signing_algorithm in get_all_enabled_signing_algorithms_supported() {
                    match signing_algorithm {
                        Algorithm::EdDSA => {
                            let public_key_jwk: Jwk = stronghold_storage
                                .get_ed25519_public_key(&KeyId::new(ED25519_KEY_ID))
                                .await
                                .unwrap();
                            public_key_jwks.push(public_key_jwk);
                        }
                        Algorithm::ES256 => {
                            let public_key_jwk: Jwk = stronghold_storage
                                .get_es256_public_key(&KeyId::new(ES256_KEY_ID))
                                .await
                                .unwrap();
                            public_key_jwks.push(public_key_jwk);
                        }
                        _ => return Err(UnsupportedSigningAlgorithmError(signing_algorithm)),
                    }
                }

                // Remove all the current Verification Methods from the Document.
                let current_verification_method_ids = document
                    .methods(None)
                    .iter()
                    .map(|method| method.id().clone())
                    .collect::<Vec<_>>();
                for method_id in current_verification_method_ids {
                    document.remove_method(&method_id);
                }

                // Add the new Verification Methods to the Document.
                for public_key_jwk in public_key_jwks {
                    let fragment = public_key_jwk.kid().ok_or(MissingKidError)?;
                    let algorithm = public_key_jwk.alg().ok_or(MissingAlgError)?.to_string();

                    let verification_method_id = did
                        .to_url()
                        .join(format!("#{fragment}"))
                        .map_err(|_| InvalidDidError("Invalid fragment".to_string()))?;
                    let verification_method = VerificationMethod::builder(Default::default())
                        .id(verification_method_id.clone())
                        .controller(did.clone())
                        .type_(MethodType::JSON_WEB_KEY_2020)
                        .data(MethodData::PublicKeyJwk(public_key_jwk))
                        .build()
                        .map_err(|err| VerificationMethodBuilderError(err.to_string()))?;

                    document
                        .insert_method(verification_method, MethodScope::VerificationMethod)
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    did_methods.insert_verification_method_id(
                        &did_method,
                        &algorithm,
                        &verification_method_id.to_string(),
                    );
                }

                Ok(vec![PublicKeyJwksSet {
                    document_id: did_method.to_string(),
                    document,
                }])
            }
            SetStatus { did_method, status } => {
                let mut did_methods = services.subject.did_methods.lock().await;

                if let Some(document) = &self.document {
                    did_methods.insert(
                        &did_method,
                        Algorithms {
                            es256: Some(DocumentData {
                                did: document.id().to_string(),
                                verification_method_id: None,
                            }),
                            eddsa: Some(DocumentData {
                                did: document.id().to_string(),
                                verification_method_id: None,
                            }),
                        },
                    );
                }

                Ok(vec![StatusSet {
                    document_id: did_method.to_string(),
                    status,
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
                service
                    .set_id(format!("{subject_did}#{service_id}").parse::<DIDUrl>().unwrap())
                    .unwrap();

                // Overwrite the service if it already exists.
                document.remove_service(service.id());
                document
                    .insert_service(service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document_id, document }])
            }
            RemoveService { service_id } => {
                let document_id = self.document_id.clone();
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let subject_did = document.id();

                let service_id = format!("{subject_did}#{service_id}");

                document.remove_service(&service_id.parse::<DIDUrl>().map_err(|_| InvalidDidError(service_id))?);

                Ok(vec![ServiceRemoved { document_id, document }])
            }
            PublishDocument { did_method } => {
                let SecretManagerConfig {
                    stronghold_password: password,
                    ..
                } = config().secret_manager.clone();

                // The API endpoint of an IOTA node, e.g. Hornet.
                let api_endpoint = did_method
                    .api_endpoint()
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
                        let alias_output: AliasOutput = iota_client
                            .update_did_output(document.clone())
                            .await
                            .map_err(IotaClientError)?;

                        alias_output
                    }
                    Status::Disabled => {
                        let did: IotaDID = document.id().clone();

                        // Deactivate the DID by publishing an empty document.
                        // This process can be reversed since the Alias Output is not destroyed.
                        // Deactivation may only be performed by the state controller of the Alias Output.
                        let deactivated_output: AliasOutput =
                            iota_client.deactivate_did_output(&did).await.map_err(IotaClientError)?;

                        deactivated_output
                    }
                };

                // Because the size of the DID document increased, we have to increase the allocated storage deposit.
                // This increases the deposit amount to the new minimum.
                let rent_structure: RentStructure = iota_client.get_rent_structure().await.map_err(IotaClientError)?;
                let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
                    .with_minimum_storage_deposit(rent_structure)
                    .finish()
                    .map_err(|_| AliasOutputBuilderError)?;

                // Create a new secret manager backed by a Stronghold.
                let secret_manager: SecretManager = SecretManager::Stronghold(
                    StrongholdSecretManager::builder()
                        .password(Password::from(password))
                        .build(STRONGHOLD_PATH)
                        .map_err(|_| SecretManagerBuilderError)?,
                );

                // Publish the updated Alias Output.
                let updated_document: CoreDocument = iota_client
                    .publish_did_output(&secret_manager, alias_output)
                    .await
                    .map(CoreDocument::from)
                    .map_err(IotaClientError)?;

                Ok(vec![DocumentPublished {
                    document_id: did_method.to_string(),
                    updated_document,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        match event {
            DocumentCreated {
                document_id,
                status,
                document,
            } => {
                self.document_id = document_id;
                self.status = status;
                self.document.replace(document);
            }
            PublicKeyJwksSet { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            StatusSet { document_id, status } => {
                self.document_id = document_id;
                self.status = status;
            }
            ServiceAdded { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            ServiceRemoved { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            DocumentPublished {
                document_id,
                updated_document,
            } => {
                self.document_id = document_id;
                self.document.replace(updated_document);
            }
        }
    }
}

// For did:web
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
    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use identity_document::service::Service;
    use rstest::rstest;

    type DocumentTestFramework = TestFramework<Document>;

    // #[rstest]
    // #[serial_test::serial]
    // async fn test_create_document(document_id: String, #[future(awt)] document: CoreDocument) {
    //     DocumentTestFramework::with(IdentityServices::default())
    //         .given_no_previous_events()
    //         .when(DocumentCommand::CreateDocument {
    //             document_id: document_id.clone(),
    //         })
    //         .then_expect_events(vec![DocumentEvent::DocumentCreated {
    //             document,
    //             document_id,
    //             status: Status::SignAndValidate,
    //         }])
    // }

    // #[rstest]
    // #[serial_test::serial]
    // async fn test_add_service(
    //     document_id: String,
    //     #[future(awt)] document: CoreDocument,
    //     domain_linkage_service: Service,
    //     #[future(awt)] document_with_domain_linkage_service: CoreDocument,
    // ) {
    //     DocumentTestFramework::with(IdentityServices::default())
    //         .given(vec![DocumentEvent::DocumentCreated {
    //             document_id,
    //             document,
    //             status: Status::SignAndValidate,
    //         }])
    //         .when(DocumentCommand::AddService {
    //             service: domain_linkage_service,
    //             service_id: "FIX THIS".to_string(),
    //         })
    //         .then_expect_events(vec![DocumentEvent::ServiceAdded {
    //             document: document_with_domain_linkage_service,
    //         }])
    // }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_shared::config::SupportedDidMethod;
    use agent_shared::{
        config::{config, get_preferred_signing_algorithm},
        from_jsonwebtoken_algorithm_to_jwsalgorithm,
    };
    use identity_core::convert::FromJson;
    use identity_document::{
        document::CoreDocument,
        service::{Service, ServiceEndpoint},
    };
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn document_id() -> String {
        SupportedDidMethod::Web.to_string()
    }

    // #[fixture]
    // pub async fn document(did_method: DidMethod) -> CoreDocument {
    //     let mut secret_manager = secret_manager().await;

    //     let method_specific_parameters = matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
    //         origin: config().url.origin(),
    //     });

    //     secret_manager
    //         .produce_document(
    //             did_method,
    //             method_specific_parameters,
    //             from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
    //         )
    //         .await
    //         .unwrap()
    // }

    #[fixture]
    pub fn domain_linkage_service() -> Service {
        Service::builder(Default::default())
            .id("did:test:123#linked_domain-service".parse().unwrap())
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

    // #[fixture]
    // pub async fn document_with_domain_linkage_service(
    //     did_method: DidMethod,
    //     domain_linkage_service: Service,
    // ) -> CoreDocument {
    //     let mut secret_manager = secret_manager().await;

    //     let method_specific_parameters = matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
    //         origin: config().url.origin(),
    //     });

    //     let mut document = secret_manager
    //         .produce_document(
    //             did_method,
    //             method_specific_parameters,
    //             from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
    //         )
    //         .await
    //         .unwrap();

    //     document.insert_service(domain_linkage_service).unwrap();

    //     document
    // }
}
