use crate::domain::event_publisher::{
    aggregate::{EventPublisher, EventPublisherInner},
    config::{EventPublisherConfig, EventPublisherFilter},
};
use chrono::{DateTime, Utc};
use tracing::debug;

impl EventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn event_publisher_created(
        &mut self,
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        debug!(id = %id, "Applying EventPublisherCreated event");
        *self = EventPublisher::Configured(Box::new(EventPublisherInner {
            id,
            name,
            description,
            enabled,
            filter,
            config,
            created_at: occurred_at,
            last_modified_at: occurred_at,
        }));
    }
}
