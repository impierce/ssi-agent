use crate::{status_list::{aggregate::StatusListAggregate, event::StatusListEvent::*}};
use cqrs_es::View;
use identity_credential::credential::Status;

pub type StatusListView = StatusListAggregate;

impl View<StatusListAggregate> for StatusListAggregate {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<StatusListAggregate>) {

        // The aggregate doesn't need to save the index but the event needs it to be useful
        match &event.payload {
            &IndexAdded { id, status_list, index, status } => {
                self.id = id.clone();
                self.list = status_list.clone();
                self.full =
            }
            &IndexUpdated { id, index, status } => {
                self.id = id.clone();
            }
        }
    }
}