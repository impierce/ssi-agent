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
                self.id.clone_from(service_id);
                self.service.replace(service.clone());
                self.resource.replace(resource.clone());
            }
            LinkedVerifiablePresentationServiceCreated { service_id, service } => {
                self.id.clone_from(service_id);
                self.service.replace(service.clone());
            }
        }
    }
}
