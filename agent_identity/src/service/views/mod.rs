pub mod all_services;

use super::aggregate::Service;
use cqrs_es::{EventEnvelope, View};

pub type ServiceView = Service;
impl View<Service> for Service {
    fn update(&mut self, event: &EventEnvelope<Service>) {
        use crate::service::event::ServiceEvent::*;

        match &event.payload {
            LinkedDomainsAdded {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            }
            | LinkedDomainsCredentialsRenewed {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            } => {
                self.service_id.clone_from(service_id);
                self.service.replace(service.clone());
                self.resource.replace(resource.clone());
                self.is_deleted.clone_from(is_deleted);
                self.origins.clone_from(origins);
            }
            LinkedDomainsRemoved {
                service_id,
                service,
                resource,
                is_deleted,
                origins,
            } => {
                self.service_id.clone_from(service_id);
                self.service.clone_from(service);
                self.resource.clone_from(resource);
                self.is_deleted.clone_from(is_deleted);
                self.origins.clone_from(origins);
            }
            LinkedVerifiablePresentationsAdded {
                service_id,
                presentations,
            } => self.apply_linked_verifiable_presentations_added(service_id.clone(), presentations.clone()),
            LinkedVerifiablePresentationsRemoved {
                service_id,
                presentation_ids,
            } => self.apply_linked_verifiable_presentations_removed(service_id.clone(), presentation_ids),
        }
    }
}
