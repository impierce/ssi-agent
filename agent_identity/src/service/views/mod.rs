pub mod all_services;

use super::aggregate::Service;
use cqrs_es::{EventEnvelope, View};

pub type ServiceView = Service;
impl View<Service> for Service {
    fn update(&mut self, event: &EventEnvelope<Service>) {
        use crate::service::event::ServiceEvent::*;

        match &event.payload {
            DomainLinkageServiceCreated {
                service_id,
                service,
                resource,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
                self.resource.replace(resource.clone());
                self.is_deleted.clone_from(is_deleted);
            }
            DomainLinkageServiceDeleted {
                service_id,
                service,
                resource,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.clone_from(service);
                self.resource.clone_from(resource);
                self.is_deleted.clone_from(is_deleted);
            }
            LinkedVerifiablePresentationServiceCreated {
                service_id,
                presentation_ids,
                service,
            } => {
                self.service_id.clone_from(service_id);
                self.presentation_ids.clone_from(presentation_ids);
                self.service.replace(service.clone());
            }
            DataAccessServiceCreated {
                service_id,
                service,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
                self.is_deleted.clone_from(is_deleted);
            }
            DataAccessServiceDeleted {
                service_id,
                service,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.clone_from(service);
                self.is_deleted.clone_from(is_deleted);
            }
            PublicVerificationServiceCreated {
                service_id,
                service,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
                self.is_deleted.clone_from(is_deleted);
            }
            PublicVerificationServiceDeleted {
                service_id,
                service,
                is_deleted,
            } => {
                self.service_id.clone_from(service_id);
                self.service.clone_from(service);
                self.is_deleted.clone_from(is_deleted);
            }
        }
    }
}
