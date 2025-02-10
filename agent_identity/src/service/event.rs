use cqrs_es::DomainEvent;
use derivative::Derivative;
use identity_document::service::Service as DocumentService;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::aggregate::{ServiceResource, Status};

#[derive(Clone, Debug, Deserialize, Serialize, Derivative, Display)]
#[derivative(PartialEq)]
pub enum ServiceEvent {
    DomainLinkageServiceCreated {
        service_id: String,
        status: Status,
        service: DocumentService,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
    },
    DomainLinkageServiceDeleted {
        service_id: String,
        status: Status,
        service: Option<DocumentService>,
        #[derivative(PartialEq = "ignore")]
        resource: Option<ServiceResource>,
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
