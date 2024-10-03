use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::services::IdentityServices;
use agent_shared::{
    config::{config, get_preferred_did_method, get_preferred_signing_algorithm},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_core::{
    common::{Duration, OrderedSet, Timestamp},
    convert::{FromJson, ToJson},
};
use identity_credential::{
    credential::Jwt,
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
};
use identity_did::{CoreDID, DIDUrl};
use identity_document::service::{Service as DocumentService, ServiceEndpoint};
use jsonwebtoken::Header;
use oid4vc_core::authentication::subject::Subject as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceResource {
    DomainLinkage(DomainLinkageConfiguration),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Service {
    pub id: String,
    pub service: Option<DocumentService>,
    pub resource: Option<ServiceResource>,
}

#[async_trait]
impl Aggregate for Service {
    type Command = ServiceCommand;
    type Event = ServiceEvent;
    type Error = ServiceError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "service".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use ServiceCommand::*;
        use ServiceError::*;
        use ServiceEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateDomainLinkageService { service_id } => {
                let subject = &services.subject;

                let origin = config().url.origin();

                let subject_did = subject
                    .identifier(
                        &get_preferred_did_method().to_string(),
                        get_preferred_signing_algorithm(),
                    )
                    .await
                    .map_err(|err| MissingIdentifierError(err.to_string()))?;

                let origin = identity_core::common::Url::parse(origin.ascii_serialization())
                    .map_err(|err| InvalidUrlError(err.to_string()))?;
                let domain_linkage_credential = DomainLinkageCredentialBuilder::new()
                    .issuer(
                        subject_did
                            .parse::<CoreDID>()
                            .map_err(|err| InvalidDidError(err.to_string()))?,
                    )
                    .origin(origin.clone())
                    .issuance_date(Timestamp::now_utc())
                    // TODO: make this configurable
                    .expiration_date(
                        Timestamp::now_utc()
                            .checked_add(Duration::days(365))
                            .ok_or(InvalidTimestampError)?,
                    )
                    .build()
                    .map_err(|err| DomainLinkageCredentialBuilderError(err.to_string()))?
                    .serialize_jwt(Default::default())
                    .map_err(|err| SerializationError(err.to_string()))?;

                // Compose JWT
                let header = Header {
                    alg: get_preferred_signing_algorithm(),
                    typ: None,
                    // TODO: make dynamic
                    kid: Some(format!("{subject_did}#key-0")),
                    ..Default::default()
                };

                let message = [
                    URL_SAFE_NO_PAD.encode(
                        header
                            .to_json_vec()
                            .map_err(|err| SerializationError(err.to_string()))?,
                    ),
                    URL_SAFE_NO_PAD.encode(domain_linkage_credential.as_bytes()),
                ]
                .join(".");

                let secret_manager = subject.secret_manager.lock().await;

                let proof_value = secret_manager
                    .sign(
                        message.as_bytes(),
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    .map_err(|err| SigningError(err.to_string()))?;
                let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
                let message = [message, signature].join(".");

                let domain_linkage_configuration = DomainLinkageConfiguration::new(vec![Jwt::from(message)]);
                info!("Configuration Resource >>: {domain_linkage_configuration:#}");

                // Create a new service and add it to the DID document.
                let service = DocumentService::builder(Default::default())
                    .id(format!("{subject_did}#{service_id}")
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidUrlError(err.to_string()))?)
                    .type_("LinkedDomains")
                    .service_endpoint(
                        ServiceEndpoint::from_json_value(json!({
                            "origins": [origin]
                        }))
                        .map_err(|err| InvalidServiceEndpointError(err.to_string()))?,
                    )
                    .build()
                    .expect("Failed to create DID Configuration Resource");

                Ok(vec![DomainLinkageServiceCreated {
                    service_id,
                    service,
                    resource: ServiceResource::DomainLinkage(domain_linkage_configuration),
                }])
            }
            CreateLinkedVerifiablePresentationService {
                service_id,
                presentation_ids,
            } => {
                let mut secret_manager = services.subject.secret_manager.lock().await;

                let origin = config().url.origin();
                let method_specific_parameters = MethodSpecificParameters::Web { origin: origin.clone() };
                let origin = identity_core::common::Url::parse(origin.ascii_serialization())
                    .map_err(|err| InvalidUrlError(err.to_string()))?;

                // TODO: implement for all non-deterministic methods and not just DID WEB
                let document = secret_manager
                    .produce_document(
                        DidMethod::Web,
                        Some(method_specific_parameters),
                        // TODO: This way the Document can only support on single algorithm. We need to support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    .map_err(|err| ProduceDocumentError(err.to_string()))?;

                let subject_did = document.id();

                let service = DocumentService::builder(Default::default())
                    .id(format!("{subject_did}#{service_id}")
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidUrlError(err.to_string()))?)
                    .type_("LinkedVerifiablePresentation")
                    .service_endpoint(ServiceEndpoint::from(OrderedSet::from_iter(
                        presentation_ids
                            .into_iter()
                            .map(|presentation_id| {
                                // TODO: Find a better way to construct the URL
                                format!("{origin}v0/holder/presentations/{presentation_id}/signed")
                                    .parse::<identity_core::common::Url>()
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|err| InvalidUrlError(err.to_string()))?,
                    )))
                    .build()
                    .expect("Failed to create Linked Verifiable Presentation Resource");

                Ok(vec![LinkedVerifiablePresentationServiceCreated { service_id, service }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ServiceEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            DomainLinkageServiceCreated {
                service_id,
                service,
                resource,
            } => {
                self.id = service_id;
                self.service.replace(service);
                self.resource.replace(resource);
            }
            LinkedVerifiablePresentationServiceCreated { service_id, service } => {
                self.id = service_id;
                self.service.replace(service);
            }
        }
    }
}

#[cfg(test)]
pub mod service_tests {
    use identity_document::service::Service as DocumentService;

    use super::test_utils::*;
    use super::*;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type ServiceTestFramework = TestFramework<Service>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_domain_linkage_service(
        domain_linkage_service_id: String,
        domain_linkage_service: DocumentService,
        domain_linkage_resource: ServiceResource,
    ) {
        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::CreateDomainLinkageService {
                service_id: domain_linkage_service_id.clone(),
            })
            .then_expect_events(vec![ServiceEvent::DomainLinkageServiceCreated {
                service_id: domain_linkage_service_id,
                service: domain_linkage_service,
                resource: domain_linkage_resource,
            }])
    }

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
    use super::*;
    use crate::state::DOMAIN_LINKAGE_SERVICE_ID;
    use agent_shared::config::config;
    use identity_core::convert::FromJson;
    use identity_document::service::{Service, ServiceEndpoint};
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn domain_linkage_service_id() -> String {
        DOMAIN_LINKAGE_SERVICE_ID.to_string()
    }

    #[fixture]
    pub fn domain_linkage_service() -> DocumentService {
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
    pub fn domain_linkage_resource() -> ServiceResource {
        let domain_linkage_configuration = DomainLinkageConfiguration::new(vec![Jwt::from("message".to_string())]);

        ServiceResource::DomainLinkage(domain_linkage_configuration)
    }
}
