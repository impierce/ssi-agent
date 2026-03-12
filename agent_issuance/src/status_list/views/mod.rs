use crate::status_list::{aggregate::StatusListAggregate, event::StatusListEvent::*};
use cqrs_es::View;

pub type StatusListView = StatusListAggregate;

impl View<StatusListAggregate> for StatusListAggregate {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<StatusListAggregate>) {
        // The aggregate doesn't need to save the index but the event needs it to be useful
        match &event.payload {
            IndexAdded {
                id,
                status_list,
                index: _,
                status: _,
                used_indices,
            } => {
                self.id.clone_from(id);
                self.list.clone_from(status_list);
                self.used_indices.clone_from(used_indices);
                // self.index = index;
                // self.status = status;
            }
            IndexUpdated {
                id,
                status_list,
                index: _,
                status: _,
            } => {
                self.id.clone_from(id);
                self.list.clone_from(status_list);
                // self.index = index;
                // self.status = status;
            }
        }
    }
}
