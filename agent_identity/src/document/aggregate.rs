use std::sync::Arc;

use agent_shared::{
    config::{config, get_preferred_did_method, get_preferred_signing_algorithm, SecretManagerConfig},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_did::{CoreDID, DIDUrl, DID as _};
use identity_document::{document::CoreDocument, service::Service as DocumentService};
use identity_iota::{
    iota::{IotaClientExt as _, IotaDID, IotaDocument, IotaIdentityClientExt as _},
    storage::KeyId,
    verification::{MethodData, MethodScope, MethodType, VerificationMethod},
};
use identity_stronghold::{StrongholdKeyType, StrongholdStorage};
use iota_sdk::{
    client::{
        secret::{stronghold::StrongholdSecretManager, SecretManager},
        stronghold::StrongholdAdapter,
        Client, Password,
    },
    types::block::output::{AliasOutput, AliasOutputBuilder, RentStructure},
};
use oid4vc_core::authentication::subject::Subject as _;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::services::IdentityServices;

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
            CreateDocument { document_id, status } => {
                info!("Service ID 1: {:?}", document_id);
                info!("Creating document: {:?}", document_id);

                let mut secret_manager = services.subject.secret_manager.lock().await;

                info!("Secret Manager");
                // FIX THISS
                let did_method: DidMethod = serde_json::from_value(serde_json::json!(document_id)).unwrap();

                info!("DID Method: {:?}", did_method);

                let method_specific_parameters =
                    matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
                        origin: config().url.origin(),
                    });

                info!(
                    "Method Specific Parameters is some: {:?}",
                    method_specific_parameters.is_some()
                );

                let document = secret_manager
                    .produce_document(
                        did_method.clone(),
                        method_specific_parameters,
                        // TODO: This way the Document can only support on single algorithm. We need to make sure that
                        // Documents can support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    .map_err(|err| ProduceDocumentError(err.to_string()))?;

                info!("Document: {:#?}", document);

                Ok(vec![DocumentCreated {
                    document_id,
                    status,
                    document,
                }])
            }
            SetStatus { document_id, status } => {
                info!("Service ID 2: {:?}", self.document_id);
                let mut document = self.document.clone().unwrap();

                let did = document.id().clone();
                let fragment = config().secret_manager.issuer_fragment.clone().unwrap();
                let password = config().secret_manager.stronghold_password.clone();
                let stronghold_path = config().secret_manager.stronghold_path.clone();
                let issuer_eddsa_key_id = config().secret_manager.issuer_eddsa_key_id.clone().unwrap();

                let adapter = StrongholdAdapter::builder()
                    .password(password)
                    .build(stronghold_path)
                    .unwrap();

                let stronghold_storage = StrongholdStorage::new(adapter);

                let jwk: identity_iota::verification::jwk::Jwk = stronghold_storage
                    .get_public_key_with_type(&KeyId::new(issuer_eddsa_key_id), StrongholdKeyType::Ed25519)
                    .await
                    .unwrap();

                info!("DID : {}", did);
                info!("Fragment : {}", fragment);
                info!("JWK : {:#?}", jwk);

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

                let verification_method = method(&did, &format!("#{fragment}"), jwk);

                document.remove_method(&verification_method.id());
                document
                    .insert_method(verification_method, MethodScope::VerificationMethod)
                    .unwrap();

                info!("HELLOOO 2: {:#?}", document);

                Ok(vec![StatusSet {
                    document_id,
                    status,
                    document,
                }])
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
                let did_method: DidMethod = serde_json::from_value(serde_json::json!(document_id)).unwrap();

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
                let did_method: DidMethod = serde_json::from_value(serde_json::json!(document_id)).unwrap();

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
                    stronghold_path: snapshot_path,
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
                        .build(snapshot_path)
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
                let updated_document: IotaDocument = client
                    .publish_did_output(&secret_manager, alias_output)
                    .await
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
            StatusSet { status, document, .. } => {
                self.status = status;
                self.document.replace(document);
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
    use agent_secret_manager::secret_manager;
    use agent_shared::{
        config::{config, get_preferred_signing_algorithm},
        from_jsonwebtoken_algorithm_to_jwsalgorithm,
    };
    use did_manager::{DidMethod, MethodSpecificParameters};
    use identity_core::convert::FromJson;
    use identity_document::{
        document::CoreDocument,
        service::{Service, ServiceEndpoint},
    };
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn did_method() -> DidMethod {
        DidMethod::Web
    }

    #[fixture]
    pub async fn document(did_method: DidMethod) -> CoreDocument {
        let mut secret_manager = secret_manager().await;

        let method_specific_parameters = matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
            origin: config().url.origin(),
        });

        secret_manager
            .produce_document(
                did_method,
                method_specific_parameters,
                from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
            )
            .await
            .unwrap()
    }

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

    #[fixture]
    pub async fn document_with_domain_linkage_service(
        did_method: DidMethod,
        domain_linkage_service: Service,
    ) -> CoreDocument {
        let mut secret_manager = secret_manager().await;

        let method_specific_parameters = matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
            origin: config().url.origin(),
        });

        let mut document = secret_manager
            .produce_document(
                did_method,
                method_specific_parameters,
                from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
            )
            .await
            .unwrap();

        document.insert_service(domain_linkage_service).unwrap();

        document
    }
}
