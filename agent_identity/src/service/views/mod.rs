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
                type_,
                service_endpoint,
                resource,
            } => {
                self.service_id.clone_from(service_id);
                self.type_.replace(type_.clone());
                self.service_endpoint.replace(service_endpoint.clone());
                self.resource.replace(resource.clone());
            }
            LinkedVerifiablePresentationServiceCreated {
                service_id,
                presentation_ids,
                type_,
                service_endpoint,
            } => {
                self.service_id.clone_from(service_id);
                self.presentation_ids.clone_from(presentation_ids);
                self.type_.replace(type_.clone());
                self.service_endpoint.replace(service_endpoint.clone());
            }
        }
    }
}
