use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};
use crate::{services::IdentityServices, state::get_address};
use agent_secret_manager::StorageKey;
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{config, get_all_enabled_signing_algorithms_supported, SecretManagerConfig};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::document::CoreDocument;
use identity_iota::core::FromJson as _;
use identity_iota::verification;
use identity_iota::{
    iota::{IotaClientExt as _, IotaDID, IotaDocument, IotaIdentityClientExt as _, NetworkName},
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
use ssi_dids::DIDMethod;
use ssi_dids::Source;
use std::collections::HashMap;
use std::{collections::BTreeMap, str::FromStr as _, sync::Arc};
use tracing::{debug, info};

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

async fn destroy_did(secret_manager: &SecretManager, iota_client: &Client) {
    {
        let address =
            Address::try_from_bech32("iota1qp9tv23cdmc67ueg342wm6dl0p5cza9mpyxvgd7xx4fwxgu34dsf7z854ml").unwrap();

        let did: IotaDID = "did:iota:0x1a25b794d9e7f5a571631976e06c6798ddd8ff55dd4284aba708262b006db8f0"
            .parse()
            .unwrap();

        iota_client
            .delete_did_output(secret_manager, address, &did)
            .await
            .unwrap();

        info!("here 6");
    }
}

fn insert_did_key(storage: &mut HashMap<(SupportedDidMethod, Algorithm), DIDUrl>) {}

fn insert_did_iota(storage: &mut HashMap<(SupportedDidMethod, Algorithm), DIDUrl>) {
    let es256_verification_method_id: DIDUrl = "did:iota:0xac31b2d282456855d81905cfc5f69d2d66f7ce5934102769f94c49445975b848#p7Ql29pWlNjXUWDQ3QyUsJ6bc5-Q4FFDpr5WxU4SmnM"
        .parse()
        .unwrap();
    let eddsa_verification_method_id: DIDUrl = "did:iota:0xac31b2d282456855d81905cfc5f69d2d66f7ce5934102769f94c49445975b848#p7Ql29pWlNjXUWDQ3QyUsJ6bc5-Q4FFDpr5WxU4SmnM"
    .parse()
    .unwrap();

    storage.insert(
        (SupportedDidMethod::Iota, Algorithm::ES256),
        es256_verification_method_id,
    );
    storage.insert(
        (SupportedDidMethod::Iota, Algorithm::EdDSA),
        eddsa_verification_method_id,
    );
}

#[test]
fn test() {
    let mut storage = HashMap::new();
    insert_did_iota(&mut storage);

    println!("{storage:#?}");
}

#[test]
fn test2() {
    let mut document = CoreDocument::builder(Default::default())
        .id(CoreDID::parse("did:test:123").unwrap())
        .build()
        .unwrap();

    let public_key_jwk: Jwk = serde_json::from_value(serde_json::json!({
      "alg": "ES256",
      "crv": "P-256",
      "kid": "iPXQXjLORRIs5WzY6EglyPyLQo2NSDYAJnNwlXimBiw",
      "kty": "EC",
      "x": "_hIiWckbRPT3pkNlEDkzhvQI8u6jyJ9M_m3gl2CT31g",
      "y": "PjYVkcI9O6sLm3snDQFLOMhYxJUURmCJFWo1QGoiKdk"
    }))
    .unwrap();

    let verification_method =
        VerificationMethod::new_from_jwk(CoreDID::parse("did:test:123").unwrap(), public_key_jwk, Some("0")).unwrap();

    document
        .insert_method(verification_method, MethodScope::VerificationMethod)
        .unwrap();

    println!("{}", serde_json::to_string_pretty(&document).unwrap());
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
                let stronghold_manager = &services.subject.stronghold_manager;
                let stronghold_storage = &stronghold_manager.stronghold_storage;
                // let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();
                // let es256_key_id = config().secret_manager.issuer_es256_key_id.clone();

                // let public_key_jwk = match signing_algorithm {
                //     Some(Algorithm::EdDSA) => {
                //         let public_key_jwk = json!(stronghold_storage
                //             .get_ed25519_public_key(&ed25519_key_id)
                //             .await
                //             .expect("Could not find EdDSA public key"));

                //         Some(public_key_jwk)
                //     }
                //     Some(Algorithm::ES256) => {
                //         let public_key_jwk = json!(stronghold_storage
                //             .get_es256_public_key(&es256_key_id)
                //             .await
                //             .expect("Could not find ES256 public key"));

                //         Some(public_key_jwk)
                //     }
                //     None => None,
                //     _ => {
                //         // FIX THIS
                //         panic!("Unsuported algorithm");
                //     }
                // };

                let document = match &did_method {
                    SupportedDidMethod::Iota | SupportedDidMethod::IotaSmr | SupportedDidMethod::IotaRms => {
                        // The API endpoint of an IOTA node, e.g. Hornet.
                        let api_endpoint = did_method
                            .api_endpoint()
                            .ok_or_else(|| InvalidNodeEndpointError("missing `api_endpoint`".to_string()))?;

                        info!("here 1");

                        // Create a new client to interact with the IOTA ledger.
                        let iota_client: Client = Client::builder()
                            .with_node(api_endpoint)
                            .map_err(|_| InvalidNodeEndpointError(api_endpoint.to_string()))?
                            .finish()
                            .await
                            .map_err(|err| IotaClientBuilderError(err.to_string()))?;

                        info!("here 2");

                        let address: Bech32Address = get_address(&iota_client, stronghold_storage.as_secret_manager())
                            .await
                            .map_err(|err| SecretManagerInitializationError(err.to_string()))?;

                        info!("here 3");
                        info!("Address 1: {:?}", address);

                        // {
                        //     let ledger_sponsoring_service = config().external_services.clone().and_then(|external_services| external_services.clone().ledger_sponsoring).expect(
                        //     "Ledger sponsoring service not configured. Please configure the `ledger_sponsoring` in the config file.",
                        // );
                        //     let access_key = ledger_sponsoring_service.access_key;
                        //     let url = ledger_sponsoring_service.url;

                        //     let client = reqwest::Client::new();

                        //     let json = json!({
                        //         "RequestSponsoring": {
                        //             "access_key": access_key,
                        //             "amount": 200000,
                        //             "address": address
                        //         }
                        //     });

                        //     // TODO: remove this once the ledger sponsoring service does not require authorization anymore.
                        //     let authorization = ledger_sponsoring_service.authorization;
                        //     let mut headers = HeaderMap::new();
                        //     headers.insert("Authorization", authorization.parse().expect("Invalid authorization"));

                        //     info!("Requesting funds for address: `{}`", address);

                        //     let _ = client
                        //         .request(Method::POST, url)
                        //         .headers(headers)
                        //         .json(&json)
                        //         .send()
                        //         .await;
                        // }

                        // TODO: poll the ledger until the address is sponsored.
                        std::thread::sleep(std::time::Duration::from_secs(0));

                        let address: Address = *address;
                        info!("Address 2: {:?}", address);

                        let network_name: NetworkName = iota_client.network_name().await.map_err(IotaClientError)?;
                        let document: IotaDocument = IotaDocument::new(&network_name);

                        info!("here 4");

                        // Construct an Alias Output containing the DID document, with the wallet address
                        // set as both the state controller and governor.
                        let alias_output: AliasOutput = iota_client
                            .new_did_output(address, document, None)
                            .await
                            .map_err(IotaClientError)?;

                        info!("here 5");

                        // Publish the Alias Output and get the published DID document.
                        let document: IotaDocument = iota_client
                            .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                            .await
                            .map_err(IotaClientError)?;

                        // destroy_did(stronghold_storage.as_secret_manager(), &iota_client).await;
                        // todo!();

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
                    SupportedDidMethod::Jwk | SupportedDidMethod::Key => {
                        let (key_id, algorithm) = match with_fixed_algorithm {
                            Some(algorithm) if algorithm == Algorithm::EdDSA => {
                                (config().secret_manager.issuer_eddsa_key_id.clone(), algorithm)
                            }
                            Some(algorithm) if algorithm == Algorithm::ES256 => {
                                (config().secret_manager.issuer_es256_key_id.clone(), algorithm)
                            }
                            _ => todo!(),
                        };

                        let public_key_jwk = stronghold_manager.get_public_key(key_id, &algorithm).await.unwrap();
                        let jwk: ssi_jwk::JWK = serde_json::from_value(json!(public_key_jwk)).unwrap();

                        let did = match did_method {
                            SupportedDidMethod::Jwk => did_jwk_extern::DIDJWK.generate(&Source::Key(&jwk)).unwrap(),
                            SupportedDidMethod::Key => did_key_extern::DIDKey.generate(&Source::Key(&jwk)).unwrap(),
                            _ => unreachable!(),
                        };

                        CoreDocument::builder(Default::default())
                            .id(CoreDID::parse(did).unwrap())
                            .build()
                            .map_err(|err| ProduceDocumentError(err.to_string()))?
                    }
                };

                let status = Status::SignAndValidate;

                let controller = document.id();
                let event = if controller == &"did:test:FIXTHISS".parse::<CoreDID>().unwrap() {
                    // FIX THIS: to warn! or not if not compatible with Stornghold?
                    DocumentUpdated {
                        document_id,
                        // FIX THISS: delete this?
                        status,
                        document,
                        with_fixed_algorithm,
                    }
                } else {
                    DocumentCreated {
                        document_id,
                        did_method,
                        status,
                        document,
                        with_fixed_algorithm,
                    }
                };

                Ok(vec![event])
            }
            UpdatePublicKeys {
                document_id,
                // TODO: decide whether the public keys should be suplied through the command or not.
                public_key_jwks: _,
            } => {
                let mut document = self
                    .document
                    .clone()
                    .ok_or_else(|| ProduceDocumentError(document_id.clone()))?;

                let did = document.id().clone();

                let stronghold_manager = &services.subject.stronghold_manager;

                let mut events = vec![];
                for signing_algorithm in self
                    .with_fixed_algorithm
                    .map(|signing_algorithm| vec![signing_algorithm])
                    .unwrap_or_else(|| get_all_enabled_signing_algorithms_supported())
                {
                    let key_id = match signing_algorithm {
                        Algorithm::EdDSA => config().secret_manager.issuer_eddsa_key_id.clone(),
                        Algorithm::ES256 => config().secret_manager.issuer_es256_key_id.clone(),
                        _ => todo!(),
                    };

                    let public_key_jwk = stronghold_manager
                        .get_public_key(key_id, &signing_algorithm)
                        .await
                        .unwrap();

                    let verification_method = VerificationMethod::new_from_jwk(
                        did.clone(),
                        public_key_jwk,
                        self.did_method.as_ref().and_then(SupportedDidMethod::fragment),
                    )
                    .unwrap();

                    stronghold_manager.insert(
                        StorageKey::new(self.did_method.unwrap(), signing_algorithm),
                        verification_method.id().clone(),
                    );

                    document
                        .insert_method(verification_method, MethodScope::VerificationMethod)
                        .map_err(|err| VerificationMethodInsertionError(err.to_string()))?;

                    events.push(PublicKeyUpdated {
                        document_id: document_id.clone(),
                        document: document.clone(),
                    })
                }

                Ok(events)
            }
            UpdateDocumentStatus { document_id, status } => Ok(vec![DocumentStatusUpdated { document_id, status }]),
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
            RemoveService { service_id } => {
                let document_id = self.document_id.clone();
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;
                let subject_did = document.id();

                let service_id = format!("{subject_did}#{service_id}");

                document.remove_service(
                    &service_id
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidDidError(err.to_string()))?,
                );

                Ok(vec![ServiceRemoved { document_id, document }])
            }
            PublishDocument { document_id } => {
                let SecretManagerConfig {
                    stronghold_password: password,
                    ..
                } = config().secret_manager.clone();

                // The API endpoint of an IOTA node, e.g. Hornet.
                let api_endpoint = self
                    .did_method
                    .as_ref()
                    .unwrap()
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

                let stronghold_path = config().secret_manager.stronghold_path.clone();

                // Create a new secret manager backed by a Stronghold.
                let secret_manager: SecretManager = SecretManager::Stronghold(
                    StrongholdSecretManager::builder()
                        .password(Password::from(password))
                        .build(stronghold_path)
                        .map_err(|_| SecretManagerBuilderError)?,
                );

                // Publish the updated Alias Output.
                let updated_document: CoreDocument = iota_client
                    .publish_did_output(&secret_manager, alias_output)
                    .await
                    .map(CoreDocument::from)
                    .map_err(IotaClientError)?;

                Ok(vec![DocumentPublished {
                    document_id,
                    updated_document,
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
            DocumentUpdated {
                document_id,
                status,
                document,
                with_fixed_algorithm,
            } => {
                self.document_id = document_id;
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

// #[cfg(test)]
// pub mod document_tests {
//     use crate::state::DOMAIN_LINKAGE_SERVICE_ID;

//     use super::test_utils::*;
//     use super::*;
//     use cqrs_es::test::TestFramework;
//     use identity_document::service::Service;
//     use rstest::rstest;

//     type DocumentTestFramework = TestFramework<Document>;

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_create_document(did_method: SupportedDidMethod, document: CoreDocument) {
//         DocumentTestFramework::with(IdentityServices::default())
//             .given_no_previous_events()
//             .when(DocumentCommand::CreateDocument {
//                 did_method: did_method.clone(),
//             })
//             .then_expect_events(vec![DocumentEvent::DocumentCreated {
//                 document,
//                 document_id: did_method.to_string(),
//                 status: Status::SignAndValidate,
//             }])
//     }

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_set_public_key_jwks(
//         did_method: SupportedDidMethod,
//         document: CoreDocument,
//         document_with_verification_method: CoreDocument,
//     ) {
//         DocumentTestFramework::with(IdentityServices::default())
//             .given(vec![DocumentEvent::DocumentCreated {
//                 document,
//                 document_id: did_method.to_string(),
//                 status: Status::SignAndValidate,
//             }])
//             .when(DocumentCommand::UpdatePublicKeys {
//                 did_method: did_method.clone(),
//                 public_key_jwks: vec![],
//             })
//             .then_expect_events(vec![DocumentEvent::PublicKeyUpdated {
//                 document_id: did_method.to_string(),
//                 document: document_with_verification_method,
//             }])
//     }

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_add_service(
//         did_method: SupportedDidMethod,
//         document: CoreDocument,
//         domain_linkage_service: Service,
//         document_with_verification_method: CoreDocument,
//         document_with_domain_linkage_service: CoreDocument,
//     ) {
//         DocumentTestFramework::with(IdentityServices::default())
//             .given(vec![
//                 DocumentEvent::DocumentCreated {
//                     document_id: did_method.to_string(),
//                     document,
//                     status: Status::SignAndValidate,
//                 },
//                 DocumentEvent::PublicKeyUpdated {
//                     document_id: did_method.to_string(),
//                     document: document_with_verification_method,
//                 },
//             ])
//             .when(DocumentCommand::AddService {
//                 service: domain_linkage_service,
//                 service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
//             })
//             .then_expect_events(vec![DocumentEvent::ServiceAdded {
//                 document_id: did_method.to_string(),
//                 document: document_with_domain_linkage_service,
//             }])
//     }

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_remove_service(
//         did_method: SupportedDidMethod,
//         document: CoreDocument,
//         document_with_verification_method: CoreDocument,
//         document_with_domain_linkage_service: CoreDocument,
//     ) {
//         DocumentTestFramework::with(IdentityServices::default())
//             .given(vec![
//                 DocumentEvent::DocumentCreated {
//                     document_id: did_method.to_string(),
//                     document,
//                     status: Status::SignAndValidate,
//                 },
//                 DocumentEvent::PublicKeyUpdated {
//                     document_id: did_method.to_string(),
//                     document: document_with_verification_method.clone(),
//                 },
//                 DocumentEvent::ServiceAdded {
//                     document_id: did_method.to_string(),
//                     document: document_with_domain_linkage_service,
//                 },
//             ])
//             .when(DocumentCommand::RemoveService {
//                 service_id: DOMAIN_LINKAGE_SERVICE_ID.to_string(),
//             })
//             .then_expect_events(vec![DocumentEvent::ServiceRemoved {
//                 document_id: did_method.to_string(),
//                 document: document_with_verification_method,
//             }])
//     }

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_set_status(
//         did_method: SupportedDidMethod,
//         document: CoreDocument,
//         document_with_verification_method: CoreDocument,
//         document_with_domain_linkage_service: CoreDocument,
//     ) {
//         DocumentTestFramework::with(IdentityServices::default())
//             .given(vec![
//                 DocumentEvent::DocumentCreated {
//                     document_id: did_method.to_string(),
//                     document,
//                     status: Status::SignAndValidate,
//                 },
//                 DocumentEvent::PublicKeyUpdated {
//                     document_id: did_method.to_string(),
//                     document: document_with_verification_method.clone(),
//                 },
//                 DocumentEvent::ServiceAdded {
//                     document_id: did_method.to_string(),
//                     document: document_with_domain_linkage_service,
//                 },
//                 DocumentEvent::ServiceRemoved {
//                     document_id: did_method.to_string(),
//                     document: document_with_verification_method,
//                 },
//             ])
//             .when(DocumentCommand::UpdateDocumentStatus {
//                 did_method: did_method.clone(),
//                 status: Status::Disabled,
//             })
//             .then_expect_events(vec![DocumentEvent::DocumentStatusUpdated {
//                 document_id: did_method.to_string(),
//                 status: Status::Disabled,
//             }])
//     }
// }

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
    pub fn document_with_verification_method(mut document: CoreDocument) -> CoreDocument {
        let verification_method = VerificationMethod::builder(Default::default())
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
            .unwrap();

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
