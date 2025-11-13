use cqrs_es::DomainEvent;
use derivative::Derivative;
use identity_document::service::Service as DocumentService;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::aggregate::ServiceResource;

#[derive(Clone, Debug, Deserialize, Serialize, Derivative, Display)]
#[derivative(PartialEq)]
pub enum ServiceEvent {
    DomainLinkageServiceCreated {
        service_id: String,
        service: DocumentService,
        #[derivative(PartialEq = "ignore")]
        resource: ServiceResource,
        is_deleted: bool,
    },
    DomainLinkageServiceDeleted {
        service_id: String,
        service: Option<DocumentService>,
        #[derivative(PartialEq = "ignore")]
        resource: Option<ServiceResource>,
        is_deleted: bool,
    },
    LinkedVerifiablePresentationServiceCreated {
        service_id: String,
        presentation_ids: Vec<String>,
        service: DocumentService,
    },
    PublicCredentialServiceCreated {
        service_id: String,
        service: DocumentService,
        is_deleted: bool,
    },
    PublicCredentialServiceDeleted {
        service_id: String,
        service: Option<DocumentService>,
        is_deleted: bool,
    },
    PublicVerificationServiceCreated {
        service_id: String,
        service: DocumentService,
        is_deleted: bool,
    },
    PublicVerificationServiceDeleted {
        service_id: String,
        service: Option<DocumentService>,
        is_deleted: bool,
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
