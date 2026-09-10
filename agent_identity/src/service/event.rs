use cqrs_es::DomainEvent;
use derivative::Derivative;
use identity_document::service::Service as DocumentService;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

use super::aggregate::ServiceResource;

#[derive(Clone, Debug, Deserialize, Serialize, Derivative, Display)]
#[derivative(PartialEq)]
pub enum ServiceEvent {
    LinkedDomainsAdded {
        service_id: String,
        service: DocumentService,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
        is_deleted: bool,
        /// Every origin linked after this event, sorted and deduplicated.
        origins: Vec<Url>,
    },
    LinkedDomainsCredentialsRenewed {
        service_id: String,
        service: DocumentService,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
        is_deleted: bool,
        origins: Vec<Url>,
    },
    /// Emitted for both a partial removal, which keeps the remaining origins' credentials, and the
    /// removal of the last origin, which leaves `service` and `resource` empty and `is_deleted` set.
    LinkedDomainsRemoved {
        service_id: String,
        service: Option<DocumentService>,
        #[derivative(PartialEq = "ignore")]
        resource: Option<ServiceResource>,
        is_deleted: bool,
        origins: Vec<Url>,
    },
    LinkedVerifiablePresentationServiceDeleted {
        service_id: String,
    },
    LinkedVerifiablePresentationServiceCreated {
        service_id: String,
        presentation_ids: Vec<String>,
        service: DocumentService,
    },
}

impl DomainEvent for ServiceEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
