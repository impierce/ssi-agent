use cqrs_es::DomainEvent;
use derivative::Derivative;
use identity_document::service::{Service as DocumentService, ServiceEndpoint};
use serde::{Deserialize, Serialize};

use super::aggregate::ServiceResource;

#[derive(Clone, Debug, Deserialize, Serialize, Derivative)]
#[derivative(PartialEq)]
pub enum ServiceEvent {
    DomainLinkageServiceCreated {
        service_id: String,
        type_: String,
        service_endpoint: ServiceEndpoint,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
    },
    LinkedVerifiablePresentationServiceCreated {
        service_id: String,
        presentation_ids: Vec<String>,
        type_: String,
        service_endpoint: ServiceEndpoint,
    },
}

impl DomainEvent for ServiceEvent {
    fn event_type(&self) -> String {
        use ServiceEvent::*;

        let event_type: &str = match self {
            DomainLinkageServiceCreated { .. } => "DomainLinkageServiceCreated",
            LinkedVerifiablePresentationServiceCreated { .. } => "LinkedVerifiablePresentationServiceCreated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
