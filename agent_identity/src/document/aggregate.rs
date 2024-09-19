use std::sync::Arc;

use agent_shared::{
    config::{config, get_preferred_signing_algorithm},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_document::document::CoreDocument;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::services::IdentityServices;

use super::{command::DocumentCommand, error::DocumentError, event::DocumentEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Document {
    pub document: Option<CoreDocument>,
}

#[async_trait]
impl Aggregate for Document {
    type Command = DocumentCommand;
    type Event = DocumentEvent;
    type Error = DocumentError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use DocumentCommand::*;
        use DocumentError::*;
        use DocumentEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateDocument { did_method } => {
                let mut secret_manager = services.subject.secret_manager.lock().await;

                let method_specific_parameters =
                    matches!(did_method, DidMethod::Web).then(|| MethodSpecificParameters::Web {
                        origin: config().url.origin(),
                    });

                let document = secret_manager
                    .produce_document(
                        did_method,
                        method_specific_parameters,
                        // TODO: This way the Document can only support on single algorithm. We need to make sure that
                        // Documents can support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    .map_err(|err| ProduceDocumentError(err.to_string()))?;

                Ok(vec![DocumentCreated { document }])
            }
            AddService { service } => {
                let mut document = self.document.clone().ok_or(MissingDocumentError)?;

                document
                    .insert_service(service)
                    .map_err(|err| AddServiceError(err.to_string()))?;

                Ok(vec![ServiceAdded { document }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DocumentEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            DocumentCreated { document } => {
                self.document.replace(document);
            }
            ServiceAdded { document } => {
                self.document.replace(document);
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

    #[rstest]
    #[serial_test::serial]
    async fn test_create_document(did_method: DidMethod, #[future(awt)] document: CoreDocument) {
        DocumentTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(DocumentCommand::CreateDocument { did_method })
            .then_expect_events(vec![DocumentEvent::DocumentCreated { document }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_service(
        #[future(awt)] document: CoreDocument,
        domain_linkage_service: Service,
        #[future(awt)] document_with_domain_linkage_service: CoreDocument,
    ) {
        DocumentTestFramework::with(IdentityServices::default())
            .given(vec![DocumentEvent::DocumentCreated { document }])
            .when(DocumentCommand::AddService {
                service: domain_linkage_service,
            })
            .then_expect_events(vec![DocumentEvent::ServiceAdded {
                document: document_with_domain_linkage_service,
            }])
    }
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
            .id(format!("did:test:123#linked_domain-service").parse().unwrap())
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
