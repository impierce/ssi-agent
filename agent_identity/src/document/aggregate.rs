use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use agent_secret_manager::{
    subject::{Algorithms, DocumentData},
    ED25519_KEY_ID, ES256_KEY_ID, STRONGHOLD_PATH,
};
use agent_shared::config::SupportedDidMethod;
use agent_shared::config::{
    config, get_all_enabled_signing_algorithms_supported, get_preferred_signing_algorithm, SecretManagerConfig,
};
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
use oid4vc_core::authentication::subject::Subject as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info};

use crate::{services::IdentityServices, state::get_address};

use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Status {
    SignAndValidate,
    ValidateOnly,
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
            CreateDocument { document_id } => {
                info!("Service ID 1: {:?}", document_id);
                info!("Creating document: {:?}", document_id);

                // let mut secret_manager = services.subject.secret_manager.lock().await;
                let stronghold_storage = &services.subject.stronghold_storage;
                let mut did_methods = services.subject.did_methods.lock().await;

                let document = match document_id.as_str() {
                    "did:iota:rms" => {
                        // The API endpoint of an IOTA node, e.g. Hornet.
                        let api_endpoint: &str = "https://api.testnet.shimmer.network";

                        // Create a new client to interact with the IOTA ledger.
                        let client: Client = Client::builder()
                            .with_primary_node(api_endpoint, None)
                            .unwrap()
                            .finish()
                            .await
                            .unwrap();

                        let address: Bech32Address = get_address(&client, stronghold_storage.as_secret_manager())
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
                        let document: IotaDocument = client
                            .publish_did_output(stronghold_storage.as_secret_manager(), alias_output)
                            .await
                            .unwrap();

                        CoreDocument::from(document)
                    }
                    "did:web" => {
                        let origin = config().url.origin();

                        debug!("Origin: {}", &origin.ascii_serialization());

                        let (_scheme, host, port) = match origin {
                            url::Origin::Tuple(ref scheme, ref host, ref port) => (scheme, host, port),
                            url::Origin::Opaque(_) => {
                                // return Err(ProducerError::Generic("Opaque origin not supported".to_string()));
                                panic!("FIX THIS");
                            }
                        };

                        // IP addresses are not allowed
                        match host {
                            url::Host::Domain(_) => {}
                            url::Host::Ipv4(_) => {
                                // return Err(ProducerError::Generic("IPv4 address not allowed".to_string()));
                                panic!("FIX THIS");
                            }
                            url::Host::Ipv6(_) => {
                                // return Err(ProducerError::Generic("IPv6 address not allowed".to_string()));
                                panic!("FIX THIS");
                            }
                        }

                        // Omit default HTTPS port
                        let host_port_encoded = match port {
                            443 => host.to_string(),
                            _ => urlencoding::encode(format!("{}:{}", host, port).as_str()).to_string(),
                        };

                        let did_str = format!("did:web:{}", host_port_encoded);

                        let controller = CoreDID::parse(did_str).unwrap();

                        // Patch the generated DID document since it's not according to spec.
                        let properties = get_properties(MethodType::JSON_WEB_KEY_2020);

                        let document = CoreDocument::builder(properties).id(controller).build().unwrap();

                        document
                    }
                    _ => {
                        panic!("FIX THIS")
                    }
                };

                did_methods.insert(
                    &document_id,
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
                document_id,
                public_key_jwks,
            } => {
                let mut document = self.document.clone().unwrap();
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
                        _ => panic!("Unsupported signing algorithm"),
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

                fn method(
                    controller: &CoreDID,
                    fragment: &str,
                    jwk: identity_iota::verification::jwk::Jwk,
                ) -> VerificationMethod {
                    VerificationMethod::builder(Default::default())
                        .id(controller.to_url().join(fragment).unwrap())
                        .controller(controller.clone())
                        .type_(MethodType::JSON_WEB_KEY_2020)
                        .data(MethodData::PublicKeyJwk(jwk))
                        .build()
                        .unwrap()
                }

                // Add the new Verification Methods to the Document.
                for public_key_jwk in public_key_jwks {
                    let fragment = public_key_jwk.kid().unwrap();
                    let algorithm = public_key_jwk.alg().unwrap().to_string();

                    let verification_method = method(&did, &format!("#{fragment}"), public_key_jwk);
                    let verification_method_id = verification_method.id().clone();

                    document
                        .insert_method(verification_method, MethodScope::VerificationMethod)
                        .unwrap();

                    did_methods.insert_verification_method_id(
                        &document_id,
                        &algorithm,
                        &verification_method_id.to_string(),
                    );
                }

                Ok(vec![PublicKeyJwksSet { document_id, document }])
            }
            SetStatus { document_id, status } => {
                info!("Service ID 2: {:?}", self.document_id);

                let mut did_methods = services.subject.did_methods.lock().await;

                if let Some(document) = &self.document {
                    did_methods.insert(
                        &document_id,
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

                Ok(vec![StatusSet { document_id, status }])
            }
            AddService {
                service_id,
                mut service,
            } => {
                info!("Service ID 3: {:?}", self.document_id);

                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                info!("HELLOOO 3: {:#?}", document);

                // FIX THISS
                let document_id = self.document_id.clone();
                let did_method = SupportedDidMethod::from_str(&document_id).unwrap();

                let subject = &services.subject;
                let subject_did = subject
                    // FIX THIS
                    .identifier(&did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .unwrap();

                // Set the service ID.
                service
                    .set_id(format!("{subject_did}#{service_id}").parse::<DIDUrl>().unwrap())
                    .unwrap();

                // Overwrite the service if it already exists.
                document.remove_service(service.id());
                document
                    .insert_service(service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document }])
            }
            RemoveService { service_id } => {
                info!("Service ID 4: {:?}", self.document_id);
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                // FIX THISS
                let document_id = self.document_id.clone();
                let did_method = SupportedDidMethod::from_str(&document_id).unwrap();

                let subject = &services.subject;
                let subject_did = subject
                    // FIX THIS
                    .identifier(&did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .unwrap();

                document.remove_service(
                    &format!("{subject_did}#{service_id}").parse::<DIDUrl>().unwrap(),
                    // .map_err(|err| InvalidUrlError(err.to_string()))?
                );

                Ok(vec![ServiceRemoved { document }])
            }
            PublishDocument { document_id } => {
                info!("Service ID 5: {:?}", self.document_id);

                let SecretManagerConfig {
                    stronghold_password: password,
                    ..
                } = config().secret_manager.clone();

                // The API endpoint of an IOTA node, e.g. Hornet.
                let api_endpoint: &str = "https://api.testnet.shimmer.network";

                // Create a new client to interact with the IOTA ledger.
                let client: Client = Client::builder()
                    .with_primary_node(api_endpoint, None)
                    .expect("FIX THIS")
                    .finish()
                    .await
                    .expect("FIX THIS");

                // Create a new secret manager backed by a Stronghold.
                let secret_manager: SecretManager = SecretManager::Stronghold(
                    StrongholdSecretManager::builder()
                        .password(Password::from(password))
                        .build(STRONGHOLD_PATH)
                        .expect("FIX THIS"),
                );

                // Resolve the latest state of the document.
                let document: IotaDocument = self.document.as_ref().ok_or(MissingDocumentError)?.clone().into();

                info!("HELLO document: {:#?}", document);

                let alias_output = match self.status {
                    Status::SignAndValidate | Status::ValidateOnly => {
                        // Resolve the latest output and update it with the given document.
                        let alias_output: AliasOutput =
                            client.update_did_output(document.clone()).await.expect("FIX THIS");

                        alias_output
                    }
                    Status::Disabled => {
                        let did: IotaDID = document.id().clone();

                        // Deactivate the DID by publishing an empty document.
                        // This process can be reversed since the Alias Output is not destroyed.
                        // Deactivation may only be performed by the state controller of the Alias Output.
                        let deactivated_output: AliasOutput =
                            client.deactivate_did_output(&did).await.expect("FIX THIS");

                        deactivated_output
                    }
                };

                // Because the size of the DID document increased, we have to increase the allocated storage deposit.
                // This increases the deposit amount to the new minimum.
                let rent_structure: RentStructure = client.get_rent_structure().await.expect("FIX THIS");
                let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
                    .with_minimum_storage_deposit(rent_structure)
                    .finish()
                    .expect("FIX THIS");

                // Publish the updated Alias Output.
                let updated_document: CoreDocument = client
                    .publish_did_output(&secret_manager, alias_output)
                    .await
                    .map(CoreDocument::from)
                    .expect("FIX THIS");
                info!("Updated DID document: {updated_document:#}");

                Ok(vec![DocumentPublished {
                    document_id,
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
            PublicKeyJwksSet { document, .. } => {
                self.document.replace(document);
            }
            StatusSet { status, .. } => {
                self.status = status;
            }
            ServiceAdded { document } => {
                self.document.replace(document);
            }
            ServiceRemoved { document } => {
                self.document.replace(document);
            }
            DocumentPublished {
                document_id,
                updated_document,
            } => {
                self.document_id = document_id;
                self.document.replace(CoreDocument::from(updated_document));
            }
        }
    }
}

// for did:web
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
    // async fn test_create_document(did_method: DidMethod, #[future(awt)] document: CoreDocument) {
    //     DocumentTestFramework::with(IdentityServices::default())
    //         .given_no_previous_events()
    //         .when(DocumentCommand::CreateDocument { did_method })
    //         .then_expect_events(vec![DocumentEvent::DocumentCreated { document }])
    // }

    // #[rstest]
    // #[serial_test::serial]
    // async fn test_add_service(
    //     #[future(awt)] document: CoreDocument,
    //     domain_linkage_service: Service,
    //     #[future(awt)] document_with_domain_linkage_service: CoreDocument,
    // ) {
    //     DocumentTestFramework::with(IdentityServices::default())
    //         .given(vec![DocumentEvent::DocumentCreated { document }])
    //         .when(DocumentCommand::AddService {
    //             service: domain_linkage_service,
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
    pub fn did_method() -> SupportedDidMethod {
        SupportedDidMethod::Web
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
