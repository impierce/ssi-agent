pub mod all_status_lists;

use crate::status_list::{aggregate::StatusListAggregate, event::StatusListEvent::*};
use cqrs_es::View;

pub type StatusListView = StatusListAggregate;

impl View<StatusListAggregate> for StatusListAggregate {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<StatusListAggregate>) {
        println!("Received event blabal");
        // The aggregate doesn't need to save the index but the event needs it to be useful
        match &event.payload {
            StatusListCreated {
                id,
                status_list,
                used_indices,
            } => {
                self.id.clone_from(id);
                self.list.clone_from(status_list);
                self.used_indices.clone_from(used_indices);
            }
            IndexAdded {
                id,
                status_list,
                used_indices,
                index: _,
                status: _,
            } => {
                self.id.clone_from(id);
                self.list.clone_from(status_list);
                self.used_indices.clone_from(used_indices);
            }
            IndexUpdated {
                id,
                status_list,
                index: _,
                status: _,
            } => {
                println!("Index updated event received for status list with id: {}", id);
                self.id.clone_from(id);
                self.list.clone_from(status_list);
            }
        }
    }
}
