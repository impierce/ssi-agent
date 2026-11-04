use crate::{
    application::views::event_publisher::EventPublisherView,
    domain::event_publisher::{aggregate::EventPublisher, event::EventPublisherEvent},
};
use serde::{Deserialize, Serialize};
use shared_kernel::cqrs_es::{EventEnvelope, View};
use std::collections::{hash_map::IntoValues, HashMap};

pub const EVENT_PUBLISHER_LIST_VIEW_ID: &str = "event-publisher-list";

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct EventPublisherListView(HashMap<String, EventPublisherView>);

impl View<EventPublisher> for EventPublisherListView {
    fn update(&mut self, event: &EventEnvelope<EventPublisher>) {
        if matches!(event.payload, EventPublisherEvent::EventPublisherDeleted { .. }) {
            self.0.remove(&event.aggregate_id);
        } else {
            self.0.entry(event.aggregate_id.clone()).or_default().update(event);
        }
    }
}

impl EventPublisherListView {
    #[must_use]
    pub fn into_values(self) -> IntoValues<String, EventPublisherView> {
        self.0.into_values()
    }
}
