use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::services::IdentityServices;
use agent_shared::config::{config, get_preferred_signing_algorithm};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use identity_core::{
    common::{Duration, OrderedSet, Timestamp},
    convert::{FromJson, ToJson},
};
use identity_credential::{
    credential::Jwt,
    domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder},
};
use identity_did::DIDUrl;
use identity_document::service::{Service as DocumentService, ServiceEndpoint};
use jsonwebtoken::Header;
use oid4vc_core::Sign as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Status {
    #[default]
    Created,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceResource {
    DomainLinkage(DomainLinkageConfiguration),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Service {
    #[serde(rename = "id")]
    pub service_id: String,
    pub status: Status,
    pub service: Option<DocumentService>,
    pub presentation_ids: Vec<String>,
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
            CreateDomainLinkageService { service_id, documents } => {
                let subject = &services.subject;

                let origin = config().url.origin();
                let origin = identity_core::common::Url::parse(origin.ascii_serialization())
                    .map_err(|err| InvalidUrlError(err.to_string()))?;

                #[cfg(feature = "test_utils")]
                let (issuance_date, expiration_date) = {
                    let issuance_date = test_utils::issuance_date();
                    let expiration_date = test_utils::expiration_date();
                    (issuance_date, expiration_date)
                };
                #[cfg(not(feature = "test_utils"))]
                let (issuance_date, expiration_date) = {
                    let issuance_date = Timestamp::now_utc();
                    let expiration_date = issuance_date
                        // TODO: make this configurable
                        .checked_add(Duration::days(365))
                        .ok_or(InvalidTimestampError)?;

                    (issuance_date, expiration_date)
                };

                // TODO: make sure that multiple signing algorithms are supported.
                let signing_algorithm = get_preferred_signing_algorithm();

                let mut messages = vec![];

                for document in documents.into_iter() {
                    let core_document = document.document.expect("FIX THIS");
                    let subject_did = core_document.id().clone();

                    // TODO: Once we support multiple algorithms we cannot simply pick the first (and only) verification
                    // method in the Document.
                    let fragment = core_document
                        .verification_method()
                        .first()
                        .expect("FIX THIS")
                        .id()
                        .fragment()
                        .expect("FIX THIS")
                        .to_string();

                    let domain_linkage_credential = DomainLinkageCredentialBuilder::new()
                        .issuer(subject_did.clone())
                        .origin(origin.clone())
                        .issuance_date(issuance_date)
                        .expiration_date(expiration_date)
                        .build()
                        .map_err(|err| DomainLinkageCredentialBuilderError(err.to_string()))?
                        .serialize_jwt(Default::default())
                        .map_err(|err| SerializationError(err.to_string()))?;

                    // Compose JWT
                    let header = Header {
                        alg: signing_algorithm.clone(),
                        typ: None,
                        // TODO: make dynamic
                        kid: Some(format!("{subject_did}#{fragment}")),
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

                    let proof_value = subject
                        .sign(message.as_str(), "FIX THIS", signing_algorithm)
                        .await
                        .map_err(|err| SigningError(err.to_string()))?;
                    let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
                    let message = [message, signature].join(".");

                    messages.push(Jwt::from(message))
                }

                if messages.is_empty() {
                    panic!("FIX THISS")
                    // return Err(NoMessagesError);
                }

                let domain_linkage_configuration = DomainLinkageConfiguration::new(messages);

                info!("Configuration Resource >>: {domain_linkage_configuration:#}");

                let type_ = "LinkedDomains".to_string();
                let service_endpoint = ServiceEndpoint::from_json_value(json!({
                    "origins": [origin]
                }))
                .map_err(|err| InvalidServiceEndpointError(err.to_string()))?;

                // Create a new service.
                let service = DocumentService::builder(Default::default())
                    .id(
                        format!("did:place:holder#{service_id}").parse::<DIDUrl>().unwrap(),
                        // .map_err(|err| InvalidUrlError(err.to_string()))?
                    )
                    .type_(type_)
                    .service_endpoint(service_endpoint)
                    .build()
                    .expect("Failed to create DID Configuration Resource");

                Ok(vec![DomainLinkageServiceCreated {
                    service_id,
                    status: Status::Created,
                    service,
                    resource: ServiceResource::DomainLinkage(domain_linkage_configuration),
                }])
            }
            DeleteDomainLinkageService { service_id } => Ok(vec![DomainLinkageServiceDeleted {
                service_id,
                status: Status::Deleted,
                service: None,
                resource: None,
            }]),
            CreateLinkedVerifiablePresentationService {
                service_id,
                presentation_ids,
            } => {
                let origin = config().url.origin();
                let origin = identity_core::common::Url::parse(origin.ascii_serialization())
                    .map_err(|err| InvalidUrlError(err.to_string()))?;

                let type_ = "LinkedVerifiablePresentation".to_string();
                let service_endpoint = ServiceEndpoint::from(OrderedSet::from_iter(
                    presentation_ids
                        .clone()
                        .into_iter()
                        .map(|presentation_id| {
                            // TODO: Find a better way to construct the URL
                            format!("{origin}linked-verifiable-presentations/{presentation_id}")
                                .parse::<identity_core::common::Url>()
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|err| InvalidUrlError(err.to_string()))?,
                ));

                // Create a new service.
                let service = DocumentService::builder(Default::default())
                    .id(
                        format!("did:place:holder#{service_id}").parse::<DIDUrl>().unwrap(),
                        // .map_err(|err| InvalidUrlError(err.to_string()))?
                    )
                    .type_(type_)
                    .service_endpoint(service_endpoint)
                    .build()
                    .expect("Failed to create DID Configuration Resource");

                Ok(vec![LinkedVerifiablePresentationServiceCreated {
                    service_id,
                    presentation_ids,
                    service,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ServiceEvent::*;

        match event {
            DomainLinkageServiceCreated {
                service_id,
                status,
                service,
                resource,
            } => {
                self.service_id = service_id;
                self.status = status;
                self.service.replace(service);
                self.resource.replace(resource);
            }
            DomainLinkageServiceDeleted {
                service_id,
                status,
                service,
                resource,
            } => {
                self.service_id = service_id;
                self.status = status;
                self.service = service;
                self.resource = resource;
            }
            LinkedVerifiablePresentationServiceCreated {
                service_id,
                service,
                presentation_ids,
            } => {
                self.service_id = service_id;
                self.presentation_ids = presentation_ids;
                self.service.replace(service);
            }
        }
    }
}

// #[cfg(test)]
// pub mod service_tests {
//     use agent_shared::config::set_config;
//     use identity_document::service::Service as DocumentService;

//     use super::test_utils::*;
//     use super::*;
//     use cqrs_es::test::TestFramework;
//     use rstest::rstest;

//     type ServiceTestFramework = TestFramework<Service>;

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_create_domain_linkage_service(
//         domain_linkage_service_id: String,
//         domain_linkage_service: DocumentService,
//         domain_linkage_resource: ServiceResource,
//     ) {
//         set_config().set_preferred_did_method(agent_shared::config::SupportedDidMethod::Web);

//         ServiceTestFramework::with(IdentityServices::default())
//             .given_no_previous_events()
//             .when(ServiceCommand::CreateDomainLinkageService {
//                 service_id: domain_linkage_service_id.clone(),
//             })
//             .then_expect_events(vec![ServiceEvent::DomainLinkageServiceCreated {
//                 service_id: domain_linkage_service_id,
//                 service: domain_linkage_service,
//                 resource: domain_linkage_resource,
//             }])
//     }

//     #[rstest]
//     #[serial_test::serial]
//     async fn test_create_linked_verifiable_presentation_service(
//         linked_verifiable_presentation_service_id: String,
//         linked_verifiable_presentation_service: DocumentService,
//     ) {
//         set_config().set_preferred_did_method(agent_shared::config::SupportedDidMethod::Web);

//         ServiceTestFramework::with(IdentityServices::default())
//             .given_no_previous_events()
//             .when(ServiceCommand::CreateLinkedVerifiablePresentationService {
//                 service_id: linked_verifiable_presentation_service_id.clone(),
//                 presentation_ids: vec!["presentation-1".to_string()],
//             })
//             .then_expect_events(vec![ServiceEvent::LinkedVerifiablePresentationServiceCreated {
//                 service_id: linked_verifiable_presentation_service_id,
//                 presentation_ids: vec!["presentation-1".to_string()],
//                 service: linked_verifiable_presentation_service,
//             }])
//     }
// }

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::state::{DOMAIN_LINKAGE_SERVICE_ID, VERIFIABLE_PRESENTATION_SERVICE_ID};
    use agent_shared::config::config;
    use identity_core::{common::Url, convert::FromJson};
    use identity_document::service::{Service, ServiceEndpoint};
    use rstest::*;
    use serde_json::json;

    #[fixture]
    pub fn domain_linkage_service_id() -> String {
        DOMAIN_LINKAGE_SERVICE_ID.to_string()
    }

    #[fixture]
    pub fn linked_verifiable_presentation_service_id() -> String {
        VERIFIABLE_PRESENTATION_SERVICE_ID.to_string()
    }

    #[fixture]
    pub fn domain_linkage_service(did_web_identifier: String, domain_linkage_service_id: String) -> DocumentService {
        Service::builder(Default::default())
            .id(format!("{did_web_identifier}#{domain_linkage_service_id}")
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
    pub fn linked_verifiable_presentation_service(
        did_web_identifier: String,
        linked_verifiable_presentation_service_id: String,
    ) -> DocumentService {
        let origin = config().url.origin().ascii_serialization();

        Service::builder(Default::default())
            .id(
                format!("{did_web_identifier}#{linked_verifiable_presentation_service_id}")
                    .parse()
                    .unwrap(),
            )
            .type_("LinkedVerifiablePresentation")
            .service_endpoint(ServiceEndpoint::from(OrderedSet::from_iter(vec![format!(
                "{origin}/linked-verifiable-presentations/presentation-1"
            )
            .parse::<Url>()
            .unwrap()])))
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn did_web_identifier() -> String {
        let domain = config().url.domain().unwrap().to_string();

        format!("did:web:{domain}")
    }

    #[fixture]
    pub fn domain_linkage_resource() -> ServiceResource {
        let domain_linkage_configuration = DomainLinkageConfiguration::new(vec![Jwt::from("eyJhbGciOiJFZERTQSIsImtpZCI6ImRpZDp3ZWI6bXktZG9tYWluLmV4YW1wbGUub3JnI2tleS0wIn0.eyJleHAiOjMxNTM2MDAwLCJpc3MiOiJkaWQ6d2ViOm15LWRvbWFpbi5leGFtcGxlLm9yZyIsIm5iZiI6MCwic3ViIjoiZGlkOndlYjpteS1kb21haW4uZXhhbXBsZS5vcmciLCJ2YyI6eyJAY29udGV4dCI6WyJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsImh0dHBzOi8vaWRlbnRpdHkuZm91bmRhdGlvbi8ud2VsbC1rbm93bi9kaWQtY29uZmlndXJhdGlvbi92MSJdLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIiwiRG9tYWluTGlua2FnZUNyZWRlbnRpYWwiXSwiY3JlZGVudGlhbFN1YmplY3QiOnsib3JpZ2luIjoiaHR0cHM6Ly9teS1kb21haW4uZXhhbXBsZS5vcmcvIn19fQ.l7dEPioa-No5zBlDCthfXDcffRB7371OnLrrQQgeAdnvHhs5F8XqRtdAWKXB8z3Se00WtGxHrTepLKmH9OWJDQ".to_string())]);

        ServiceResource::DomainLinkage(domain_linkage_configuration)
    }

    pub fn issuance_date() -> Timestamp {
        Timestamp::from_unix(0).unwrap()
    }

    pub fn expiration_date() -> Timestamp {
        issuance_date().checked_add(Duration::days(365)).unwrap()
    }
}
