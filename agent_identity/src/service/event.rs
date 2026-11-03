use cqrs_es::DomainEvent;
use identity_kit::did::Service as DocumentService;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

use super::aggregate::{LinkedVerifiablePresentation, ServiceResource};

#[derive(Clone, Debug, Deserialize, Serialize, Display)]
pub enum ServiceEvent {
    LinkedDomainsAdded {
        service_id: String,
        service: DocumentService,
        resource: ServiceResource,
        is_deleted: bool,
        /// Every origin linked after this event, sorted and deduplicated.
        origins: Vec<Url>,
    },
    LinkedDomainsCredentialsRenewed {
        service_id: String,
        service: DocumentService,
        resource: ServiceResource,
        is_deleted: bool,
        origins: Vec<Url>,
    },
    /// Emitted for both a partial removal, which keeps the remaining origins' credentials, and the
    /// removal of the last origin, which leaves `service` and `resource` empty and `is_deleted` set.
    LinkedDomainsRemoved {
        service_id: String,
        service: Option<DocumentService>,
        resource: Option<ServiceResource>,
        is_deleted: bool,
        origins: Vec<Url>,
    },
    LinkedVerifiablePresentationsAdded {
        service_id: String,
        presentations: Vec<LinkedVerifiablePresentation>,
    },
    LinkedVerifiablePresentationsRemoved {
        service_id: String,
        presentation_ids: Vec<String>,
    },
}

/// Compares every field except `resource`.
impl PartialEq for ServiceEvent {
    fn eq(&self, other: &Self) -> bool {
        use ServiceEvent::*;

        match (self, other) {
            (
                LinkedDomainsAdded {
                    service_id,
                    service,
                    resource: _,
                    is_deleted,
                    origins,
                },
                LinkedDomainsAdded {
                    service_id: other_service_id,
                    service: other_service,
                    resource: _,
                    is_deleted: other_is_deleted,
                    origins: other_origins,
                },
            )
            | (
                LinkedDomainsCredentialsRenewed {
                    service_id,
                    service,
                    resource: _,
                    is_deleted,
                    origins,
                },
                LinkedDomainsCredentialsRenewed {
                    service_id: other_service_id,
                    service: other_service,
                    resource: _,
                    is_deleted: other_is_deleted,
                    origins: other_origins,
                },
            ) => {
                (service_id, service, is_deleted, origins)
                    == (other_service_id, other_service, other_is_deleted, other_origins)
            }
            (
                LinkedDomainsRemoved {
                    service_id,
                    service,
                    resource: _,
                    is_deleted,
                    origins,
                },
                LinkedDomainsRemoved {
                    service_id: other_service_id,
                    service: other_service,
                    resource: _,
                    is_deleted: other_is_deleted,
                    origins: other_origins,
                },
            ) => {
                (service_id, service, is_deleted, origins)
                    == (other_service_id, other_service, other_is_deleted, other_origins)
            }
            (
                LinkedVerifiablePresentationsAdded {
                    service_id,
                    presentations,
                },
                LinkedVerifiablePresentationsAdded {
                    service_id: other_service_id,
                    presentations: other_presentations,
                },
            ) => (service_id, presentations) == (other_service_id, other_presentations),
            (
                LinkedVerifiablePresentationsRemoved {
                    service_id,
                    presentation_ids,
                },
                LinkedVerifiablePresentationsRemoved {
                    service_id: other_service_id,
                    presentation_ids: other_presentation_ids,
                },
            ) => (service_id, presentation_ids) == (other_service_id, other_presentation_ids),
            _ => false,
        }
    }
}

impl DomainEvent for ServiceEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}

#[cfg(test)]
mod tests {
    use identity_kit::domain_linkage::DomainLinkageConfiguration;

    use super::*;
    use crate::service::aggregate::test_utils::{linked_domains_resource, linked_domains_service};

    fn linked_domains_added(resource: ServiceResource, origins: Vec<Url>) -> ServiceEvent {
        ServiceEvent::LinkedDomainsAdded {
            service_id: "linked-domains-service".to_string(),
            service: linked_domains_service("linked-domains-service".to_string()),
            resource,
            is_deleted: false,
            origins,
        }
    }

    #[test]
    fn events_that_differ_only_in_their_resource_are_equal() {
        let origins = vec!["https://my-domain.example.org".parse().unwrap()];

        assert_eq!(
            linked_domains_added(linked_domains_resource(), origins.clone()),
            linked_domains_added(
                ServiceResource::LinkedDomains(DomainLinkageConfiguration::new()),
                origins
            ),
        );
    }

    #[test]
    fn events_that_differ_in_any_other_field_are_not_equal() {
        let event = linked_domains_added(
            linked_domains_resource(),
            vec!["https://my-domain.example.org".parse().unwrap()],
        );

        assert_ne!(
            event,
            linked_domains_added(
                linked_domains_resource(),
                vec!["https://other-domain.example.org".parse().unwrap()]
            ),
        );
        assert_ne!(
            event,
            ServiceEvent::LinkedDomainsCredentialsRenewed {
                service_id: "linked-domains-service".to_string(),
                service: linked_domains_service("linked-domains-service".to_string()),
                resource: linked_domains_resource(),
                is_deleted: false,
                origins: vec!["https://my-domain.example.org".parse().unwrap()],
            },
        );
    }
}
