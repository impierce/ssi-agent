pub mod handle;

use crate::domain::event_publisher::config::{EventPublisherConfig, EventPublisherFilter};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EventPublisherCommand {
    CreateEventPublisher {
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
    },
    UpdateEventPublisher {
        id: String,
        name: Option<String>,
        description: Option<Option<String>>,
        filter: Option<EventPublisherFilter>,
        config: Option<EventPublisherConfig>,
    },
    ActivateEventPublisher {
        id: String,
    },
    DeactivateEventPublisher {
        id: String,
    },
    DeleteEventPublisher {
        id: String,
    },
}
