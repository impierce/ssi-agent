use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::services::IdentityServices;
use agent_shared::{
    config::{config, get_preferred_signing_algorithm},
    from_jsonwebtoken_algorithm_to_jwsalgorithm,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use did_manager::{DidMethod, MethodSpecificParameters};
use identity_core::{
    common::{Duration, Timestamp},
    convert::FromJson,
};
use identity_credential::{
    credential::Jwt,
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
};
use identity_document::service::{Service as DocumentService, ServiceEndpoint};
use jsonwebtoken::Header;
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
                let mut secret_manager = services.subject.secret_manager.lock().await;

                let origin = config().url.origin();
                let method_specific_parameters = MethodSpecificParameters::Web { origin: origin.clone() };

                // TODO: implement for all non-deterministic methods and not just DID WEB
                let document = secret_manager
                    .produce_document(
                        DidMethod::Web,
                        Some(method_specific_parameters),
                        // TODO: This way the Document can only support on single algorithm. We need to support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    // FIX THISS
                    .unwrap();

                let subject_did = document.id();

                let origin = identity_core::common::Url::parse(origin.ascii_serialization()).unwrap();
                let domain_linkage_credential = DomainLinkageCredentialBuilder::new()
                    .issuer(subject_did.clone())
                    .origin(origin.clone())
                    .issuance_date(Timestamp::now_utc())
                    // Expires after a year.
                    .expiration_date(Timestamp::now_utc().checked_add(Duration::days(365)).unwrap())
                    .build()
                    // FIX THISS
                    .unwrap()
                    .serialize_jwt(Default::default())
                    // FIX THISS
                    .unwrap();

                // Compose JWT
                let header = Header {
                    alg: get_preferred_signing_algorithm(),
                    typ: Some("JWT".to_string()),
                    // TODO: make dynamic
                    kid: Some(format!("{subject_did}#key-0")),
                    ..Default::default()
                };

                let message = [
                    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap().as_slice()),
                    URL_SAFE_NO_PAD.encode(domain_linkage_credential.as_bytes()),
                ]
                .join(".");

                let proof_value = secret_manager
                    .sign(
                        message.as_bytes(),
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    .unwrap();
                let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
                let message = [message, signature].join(".");

                let domain_linkage_configuration = DomainLinkageConfiguration::new(vec![Jwt::from(message)]);
                info!("Configuration Resource >>: {domain_linkage_configuration:#}");

                // Create a new service and add it to the DID document.
                let service = DocumentService::builder(Default::default())
                    .id(format!("{subject_did}#{service_id}").parse().unwrap())
                    .type_("LinkedDomains")
                    .service_endpoint(
                        ServiceEndpoint::from_json_value(json!({
                            "origins": [origin]
                        }))
                        .unwrap(),
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
                presentation_id,
            } => {
                let mut secret_manager = services.subject.secret_manager.lock().await;

                let origin = config().url.origin();
                let method_specific_parameters = MethodSpecificParameters::Web { origin: origin.clone() };
                let origin = identity_core::common::Url::parse(origin.ascii_serialization()).unwrap();

                // TODO: implement for all non-deterministic methods and not just DID WEB
                let document = secret_manager
                    .produce_document(
                        DidMethod::Web,
                        Some(method_specific_parameters),
                        // TODO: This way the Document can only support on single algorithm. We need to support multiple algorithms.
                        from_jsonwebtoken_algorithm_to_jwsalgorithm(&get_preferred_signing_algorithm()),
                    )
                    .await
                    // FIX THISS
                    .unwrap();

                let subject_did = document.id();

                let service = DocumentService::builder(Default::default())
                    .id(format!("{subject_did}#{service_id}").parse().unwrap())
                    .type_("LinkedVerifiablePresentation")
                    .service_endpoint(ServiceEndpoint::from(
                        // FIX THIS
                        format!("{origin}v0/holder/presentations/{presentation_id}/signed")
                            .parse::<identity_core::common::Url>()
                            .unwrap(),
                    ))
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
