use crate::{
    application::views::event_publisher::EventPublisherView,
    domain::event_publisher::{aggregate::EventPublisher, event::EventPublisherEvent},
};
use serde::{Deserialize, Serialize};
use shared_kernel::cqrs_es::{EventEnvelope, View};
use std::collections::{
    hash_map::{IntoIter, IntoValues},
    HashMap,
};

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

    #[must_use]
    pub fn into_entries(self) -> IntoIter<String, EventPublisherView> {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event_publisher::config::{EventPublisherConfig, EventPublisherFilter, HttpPublisherConfig};
    use chrono::Utc;

    #[test]
    fn activated_event_updates_enabled_filter_config_and_timestamp_in_views() {
        let created_at = Utc::now();
        let activated_at = created_at + chrono::Duration::seconds(5);
        let initial_filter = EventPublisherFilter {
            types: vec!["test.initial".to_string()],
        };
        let updated_filter = EventPublisherFilter {
            types: vec!["test.updated".to_string()],
        };
        let initial_config = EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.test/v1".to_string(),
            headers: None,
        });
        let updated_config = EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.test/v2".to_string(),
            headers: None,
        });

        let mut list = EventPublisherListView::default();
        list.update(&EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 1,
            payload: EventPublisherEvent::EventPublisherCreated {
                id: "pub-1".to_string(),
                name: "Test".to_string(),
                description: None,
                enabled: false,
                filter: initial_filter,
                config: initial_config,
                occurred_at: created_at,
            },
            metadata: HashMap::new(),
        });
        list.update(&EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 2,
            payload: EventPublisherEvent::EventPublisherActivated {
                filter: updated_filter.clone(),
                config: updated_config.clone(),
                occurred_at: activated_at,
            },
            metadata: HashMap::new(),
        });

        let view = list.into_entries().next().expect("publisher entry").1;
        assert!(view.enabled);
        assert_eq!(view.filter, updated_filter);
        assert_eq!(view.config, Some(updated_config));
        assert_eq!(view.last_modified_at, activated_at);
    }
}
