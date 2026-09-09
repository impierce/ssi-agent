use super::{command::ServiceCommand, error::ServiceError, event::ServiceEvent};
use crate::services::IdentityServices;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::{event_sink::EventSink, Aggregate};
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
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::Sign as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{str::FromStr as _, sync::Arc};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceResource {
    DomainLinkage(DomainLinkageConfiguration),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Service {
    #[serde(rename = "id")]
    pub service_id: String,
    pub service: Option<DocumentService>,
    pub presentation_ids: Vec<String>,
    pub resource: Option<ServiceResource>,
    pub is_deleted: bool,
}

pub const DOMAIN_LINKAGE_VALIDITY_DAYS: u32 = 365;
pub const DOMAIN_LINKAGE_RENEWAL_WINDOW_DAYS: i64 = 30;

impl Service {
    pub fn is_active(&self) -> bool {
        self.service.is_some() && !self.is_deleted
    }

    pub fn needs_renewal(&self, now: Timestamp) -> bool {
        if !self.is_active() {
            return false;
        }
        let Some(ServiceResource::DomainLinkage(configuration)) = &self.resource else {
            return false;
        };
        let threshold = now.to_unix() + DOMAIN_LINKAGE_RENEWAL_WINDOW_DAYS * 86400;
        configuration.linked_dids().is_empty()
            || configuration.linked_dids().iter().any(|jwt| {
                let expiration = jwt
                    .as_str()
                    .split('.')
                    .nth(1)
                    .and_then(|payload| URL_SAFE_NO_PAD.decode(payload).ok())
                    .and_then(|payload| serde_json::from_slice::<serde_json::Value>(&payload).ok())
                    .and_then(|claims| claims.get("exp").and_then(serde_json::Value::as_i64));
                expiration.is_none_or(|expiration| expiration <= threshold)
            })
    }
}

impl Aggregate for Service {
    type Command = ServiceCommand;
    type Event = ServiceEvent;
    type Error = ServiceError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "service";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use ServiceCommand::*;
        use ServiceError::*;
        use ServiceEvent::*;

        info!("Handling command: {:?}", command);

        let renewal = matches!(&command, RenewDomainLinkageCredentials { .. });
        let only_if_expiring = matches!(
            &command,
            RenewDomainLinkageCredentials {
                only_if_expiring: true,
                ..
            }
        );
        let events: Vec<Self::Event> = match command {
            CreateDomainLinkageService {
                service_id,
                verification_methods,
            }
            | RenewDomainLinkageCredentials {
                service_id,
                verification_methods,
                ..
            } => {
                if renewal && !self.is_active() {
                    return Err(NotFound);
                }
                if !renewal && self.is_active() {
                    return Err(AlreadyExists);
                }
                if only_if_expiring && !self.needs_renewal((services.linkage_clock)()) {
                    return Ok(());
                }
                let subject = &services.subject;

                let origin = identity_core::common::Url::parse(services.public_url.origin().ascii_serialization())
                    .map_err(|err| InvalidUrlError(err.to_string()))?;

                let issuance_date = (services.linkage_clock)();
                let expiration_date = issuance_date
                    // TODO: make this configurable
                    .checked_add(Duration::days(DOMAIN_LINKAGE_VALIDITY_DAYS))
                    .ok_or(InvalidTimestampError)?;

                let mut linked_dids = vec![];

                // For each Verification Method, create a new linked DID JWT token.
                for verification_method in verification_methods {
                    let subject_did = verification_method.id().did();

                    let verification_method_id = verification_method.id();
                    let alg = verification_method
                        .data()
                        .public_key_jwk()
                        .and_then(|jwk| jwk.alg())
                        .ok_or_else(|| MissingVerificationMethodAlgorithm(verification_method_id.to_string()))?;
                    let algorithm = Algorithm::from_str(alg)
                        .map_err(|_| UnsupportedVerificationMethodAlgorithm(alg.to_string()))?;

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
                        alg: algorithm,
                        typ: None,
                        kid: Some(verification_method.id().to_string()),
                        ..Default::default()
                    };

                    let linked_did = [
                        URL_SAFE_NO_PAD.encode(
                            header
                                .to_json_vec()
                                .map_err(|err| SerializationError(err.to_string()))?,
                        ),
                        URL_SAFE_NO_PAD.encode(domain_linkage_credential.as_bytes()),
                    ]
                    .join(".");

                    let proof_value = subject
                        // TODO: Currently UniCore always uses the same keys for signing regardless of the DID method.
                        // Once we implement DID method-specific keys, then we should supply the appropriate
                        // `subject_syntax_type` here instead of this `placeholder` value.
                        .sign(linked_did.as_str(), "placeholder", algorithm)
                        .await
                        .map_err(|err| SigningError(err.to_string()))?;
                    let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
                    let linked_did = [linked_did, signature].join(".");

                    linked_dids.push(Jwt::from(linked_did))
                }

                if linked_dids.is_empty() {
                    return Err(EmptyLinkedDidsError);
                }

                let domain_linkage_configuration = DomainLinkageConfiguration::new(linked_dids);

                info!("Configuration Resource: {domain_linkage_configuration:#}");

                let service_endpoint = ServiceEndpoint::from_json_value(json!({
                    "origins": [origin]
                }))
                .map_err(|err| InvalidServiceEndpointError(err.to_string()))?;

                // Create a new service.
                let service = DocumentService::builder(Default::default())
                    // This service is DID method-agnostic. When added to an enabled DID Document,
                    // its placeholder value is replaced with the appropriate DID method-specific identifier.
                    .id(format!("did:place:holder#{service_id}")
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidDidError(err.to_string()))?)
                    .type_("LinkedDomains")
                    .service_endpoint(service_endpoint)
                    .build()
                    .map_err(|err| ServiceBuilderError(err.to_string()))?;

                let resource = ServiceResource::DomainLinkage(domain_linkage_configuration);
                Ok(vec![if renewal {
                    DomainLinkageCredentialsRenewed {
                        service_id,
                        service,
                        resource,
                        is_deleted: false,
                    }
                } else {
                    DomainLinkageServiceCreated {
                        service_id,
                        service,
                        resource,
                        is_deleted: false,
                    }
                }])
            }
            DeleteDomainLinkageService { service_id } => {
                if !self.is_active() {
                    return Err(NotFound);
                }
                Ok(vec![DomainLinkageServiceDeleted {
                    service_id,
                    service: None,
                    resource: None,
                    is_deleted: true,
                }])
            }
            DeleteLinkedVerifiablePresentationService { service_id } => {
                if !self.is_active() {
                    return Err(NotFound);
                }
                Ok(vec![LinkedVerifiablePresentationServiceDeleted { service_id }])
            }
            CreateLinkedVerifiablePresentationService {
                service_id,
                presentation_ids,
            } => {
                if self.is_active() {
                    return Err(AlreadyExists);
                }
                let origin = &services.public_url;

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
                    // This service is DID method-agnostic. When added to an enabled DID Document,
                    // its placeholder value is replaced with the appropriate DID method-specific identifier.
                    .id(format!("did:place:holder#{service_id}")
                        .parse::<DIDUrl>()
                        .map_err(|err| InvalidDidError(err.to_string()))?)
                    .type_("LinkedVerifiablePresentation")
                    .service_endpoint(service_endpoint)
                    .build()
                    .map_err(|err| ServiceBuilderError(err.to_string()))?;

                Ok(vec![LinkedVerifiablePresentationServiceCreated {
                    service_id,
                    presentation_ids,
                    service,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use ServiceEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            DomainLinkageServiceCreated {
                service_id,
                service,
                resource,
                is_deleted,
            }
            | DomainLinkageCredentialsRenewed {
                service_id,
                service,
                resource,
                is_deleted,
            } => {
                self.service_id = service_id;
                self.service.replace(service);
                self.resource.replace(resource);
                self.is_deleted = is_deleted;
            }
            DomainLinkageServiceDeleted {
                service_id,
                service,
                resource,
                is_deleted,
            } => {
                self.service_id = service_id;
                self.service = service;
                self.resource = resource;
                self.is_deleted = is_deleted;
            }
            LinkedVerifiablePresentationServiceDeleted { service_id } => {
                self.service_id = service_id;
                self.service = None;
                self.resource = None;
                self.presentation_ids.clear();
                self.is_deleted = true;
            }
            LinkedVerifiablePresentationServiceCreated {
                service_id,
                service,
                presentation_ids,
            } => {
                self.service_id = service_id;
                self.presentation_ids = presentation_ids;
                self.is_deleted = false;
                self.service.replace(service);
            }
        }
    }
}

#[cfg(test)]
pub mod service_tests {
    use super::test_utils::*;
    use super::*;
    use crate::document::aggregate::test_utils::both_verification_methods;
    use agent_shared::config::set_config;
    use cqrs_es::test::TestFramework;
    use identity_document::service::Service as DocumentService;
    use identity_iota::verification::VerificationMethod;
    use rstest::rstest;

    type ServiceTestFramework = TestFramework<Service>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_domain_linkage_service(
        domain_linkage_service_id: String,
        both_verification_methods: Vec<VerificationMethod>,
        domain_linkage_service: DocumentService,
        domain_linkage_resource: ServiceResource,
    ) {
        set_config().set_preferred_did_method(agent_shared::config::SupportedDidMethod::Web);

        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::CreateDomainLinkageService {
                service_id: domain_linkage_service_id.clone(),
                verification_methods: both_verification_methods,
            })
            .then_expect_events(vec![ServiceEvent::DomainLinkageServiceCreated {
                service_id: domain_linkage_service_id,
                service: domain_linkage_service,
                resource: domain_linkage_resource,
                is_deleted: false,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_domain_linkage_service(
        domain_linkage_service_id: String,
        domain_linkage_service: DocumentService,
        domain_linkage_resource: ServiceResource,
    ) {
        set_config().set_preferred_did_method(agent_shared::config::SupportedDidMethod::Web);

        ServiceTestFramework::with(IdentityServices::default())
            .given(vec![ServiceEvent::DomainLinkageServiceCreated {
                service_id: domain_linkage_service_id.clone(),
                service: domain_linkage_service.clone(),
                resource: domain_linkage_resource.clone(),
                is_deleted: false,
            }])
            .when(ServiceCommand::DeleteDomainLinkageService {
                service_id: domain_linkage_service_id.clone(),
            })
            .then_expect_events(vec![ServiceEvent::DomainLinkageServiceDeleted {
                service_id: domain_linkage_service_id,
                service: None,
                resource: None,
                is_deleted: true,
            }])
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_linked_verifiable_presentation_service(
        linked_verifiable_presentation_service_id: String,
        linked_verifiable_presentation_service: DocumentService,
    ) {
        ServiceTestFramework::with(IdentityServices::default())
            .given_no_previous_events()
            .when(ServiceCommand::CreateLinkedVerifiablePresentationService {
                service_id: linked_verifiable_presentation_service_id.clone(),
                presentation_ids: vec!["presentation-1".to_string()],
            })
            .then_expect_events(vec![ServiceEvent::LinkedVerifiablePresentationServiceCreated {
                service_id: linked_verifiable_presentation_service_id,
                presentation_ids: vec!["presentation-1".to_string()],
                service: linked_verifiable_presentation_service,
            }])
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    use cqrs_es::{EventEnvelope, View};

    #[test]
    fn persisted_service_events_rebuild_the_same_aggregate_and_projection() {
        let created = ServiceEvent::DomainLinkageServiceCreated {
            service_id: crate::state::DOMAIN_LINKAGE_SERVICE_ID.into(),
            service: test_utils::domain_linkage_service(test_utils::domain_linkage_service_id()),
            resource: test_utils::domain_linkage_resource(),
            is_deleted: false,
        };
        let removed = ServiceEvent::DomainLinkageServiceDeleted {
            service_id: crate::state::DOMAIN_LINKAGE_SERVICE_ID.into(),
            service: None,
            resource: None,
            is_deleted: true,
        };
        let mut aggregate = Service::default();
        let mut projection = Service::default();
        for (index, event) in [created.clone(), removed, created].into_iter().enumerate() {
            let persisted = serde_json::to_string(&event).unwrap();
            let restored: ServiceEvent = serde_json::from_str(&persisted).unwrap();
            aggregate.apply(restored.clone());
            projection.update(&EventEnvelope {
                aggregate_id: crate::state::DOMAIN_LINKAGE_SERVICE_ID.into(),
                sequence: index + 1,
                payload: restored,
                metadata: Default::default(),
            });
            assert_eq!(
                serde_json::to_value(&aggregate).unwrap(),
                serde_json::to_value(&projection).unwrap()
            );
            assert_eq!(aggregate.is_active(), index != 1);
        }
    }

    #[test]
    fn linked_presentation_removal_and_recreation_replay_consistently() {
        let created = ServiceEvent::LinkedVerifiablePresentationServiceCreated {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
            presentation_ids: vec!["presentation-1".into()],
            service: test_utils::linked_verifiable_presentation_service(
                test_utils::linked_verifiable_presentation_service_id(),
            ),
        };
        let removed = ServiceEvent::LinkedVerifiablePresentationServiceDeleted {
            service_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
        };
        let mut aggregate = Service::default();
        let mut projection = Service::default();
        for (index, event) in [created.clone(), removed, created].into_iter().enumerate() {
            aggregate.apply(event.clone());
            projection.update(&EventEnvelope {
                aggregate_id: crate::state::LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.into(),
                sequence: index + 1,
                payload: event,
                metadata: Default::default(),
            });
            assert_eq!(
                serde_json::to_value(&aggregate).unwrap(),
                serde_json::to_value(&projection).unwrap()
            );
            assert_eq!(aggregate.is_active(), index != 1);
        }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use crate::state::{DOMAIN_LINKAGE_SERVICE_ID, LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID};
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
        LINKED_VERIFIABLE_PRESENTATION_SERVICE_ID.to_string()
    }

    #[fixture]
    pub fn domain_linkage_service(domain_linkage_service_id: String) -> DocumentService {
        Service::builder(Default::default())
            .id(format!("did:place:holder#{domain_linkage_service_id}").parse().unwrap())
            .type_("LinkedDomains")
            .service_endpoint(
                ServiceEndpoint::from_json_value(json!({
                    "origins": ["https://my-domain.example.org/"],
                }))
                .unwrap(),
            )
            .build()
            .unwrap()
    }

    #[fixture]
    pub fn linked_verifiable_presentation_service(
        linked_verifiable_presentation_service_id: String,
    ) -> DocumentService {
        let origin = "https://my-domain.example.org";

        Service::builder(Default::default())
            .id(format!("did:place:holder#{linked_verifiable_presentation_service_id}")
                .parse()
                .unwrap())
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
