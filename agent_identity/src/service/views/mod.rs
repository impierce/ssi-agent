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
            } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
                self.resource.replace(resource.clone());
            }
            LinkedVerifiablePresentationServiceCreated { service_id, service } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
            }
        }
    }
}
