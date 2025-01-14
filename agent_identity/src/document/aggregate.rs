use std::sync::Arc;

use agent_shared::{
    config::{config, get_preferred_did_method, get_preferred_signing_algorithm, SecretManagerConfig},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_did::DIDUrl;
use identity_document::{document::CoreDocument, service::Service as DocumentService};
use identity_iota::iota::{IotaClientExt as _, IotaDocument, IotaIdentityClientExt as _};
use identity_stronghold::StrongholdStorage;
use iota_sdk::{
    client::{
        secret::{stronghold::StrongholdSecretManager, SecretManager},
        Client, Password,
    },
    types::block::output::{AliasOutput, AliasOutputBuilder, RentStructure},
};
use oid4vc_core::authentication::subject::Subject as _;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::services::IdentityServices;

use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    pub document_id: String,
    pub document: Option<CoreDocument>,
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

                Ok(vec![DocumentCreated { document_id, document }])
            }
            AddService {
                service_id,
                type_,
                service_endpoint,
            } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                // FIX THISS
                let document_id = self.document_id.clone();
                let did_method: DidMethod = serde_json::from_value(serde_json::json!(document_id)).unwrap();

                let subject = &services.subject;
                let subject_did = subject
                    .identifier(&did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .unwrap();

                // Create a new service.
                let service = DocumentService::builder(Default::default())
                    .id(
                        format!("{subject_did}#{service_id}").parse::<DIDUrl>().unwrap(),
                        // .map_err(|err| InvalidUrlError(err.to_string()))?
                    )
                    .type_(type_)
                    .service_endpoint(service_endpoint)
                    .build()
                    .expect("Failed to create DID Configuration Resource");

                // Overwrite the service if it already exists.
                document.remove_service(service.id());
                document
                    .insert_service(service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document }])
            }
            PublishDocument { document_id } => {
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

                // // FIX THIS
                // let did = self.document.as_ref().ok_or(MissingDocumentError)?.id().clone();

                // Resolve the latest state of the document.
                let document: IotaDocument = self.document.as_ref().ok_or(MissingDocumentError)?.clone().into();

                // Create a new secret manager backed by a Stronghold.
                let secret_manager: SecretManager = SecretManager::Stronghold(
                    StrongholdSecretManager::builder()
                        .password(Password::from(password))
                        .build(snapshot_path)
                        .expect("FIX THIS"),
                );

                // Resolve the latest output and update it with the given document.
                let alias_output: AliasOutput = client.update_did_output(document.clone()).await.expect("FIX THIS");

                // Because the size of the DID document increased, we have to increase the allocated storage deposit.
                // This increases the deposit amount to the new minimum.
                let rent_structure: RentStructure = client.get_rent_structure().await.expect("FIX THIS");
                let alias_output: AliasOutput = AliasOutputBuilder::from(&alias_output)
                    .with_minimum_storage_deposit(rent_structure)
                    .finish()
                    .expect("FIX THIS");

                // Publish the updated Alias Output.
                let updated: IotaDocument = client
                    .publish_did_output(&secret_manager, alias_output)
                    .await
                    .expect("FIX THIS");
                info!("Updated DID document: {updated:#}");

                Ok(vec![DocumentPublished { document_id }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            DocumentCreated { document_id, document } => {
                self.document_id = document_id;
                self.document.replace(document);
            }
            ServiceAdded { document } => {
                self.document.replace(document);
            }
            DocumentPublished { document_id } => {}
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
