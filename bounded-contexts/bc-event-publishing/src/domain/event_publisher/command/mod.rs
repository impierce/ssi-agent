pub mod handle;

use crate::domain::event_publisher::config::{EventPublisherConfig, EventPublisherFilter};
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::CommandOperation;

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

impl CommandOperation for EventPublisherCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateEventPublisher { .. } => crate::application::access::CREATE_PUBLISHER_OPERATION,
            Self::UpdateEventPublisher { .. } => crate::application::access::UPDATE_PUBLISHER_OPERATION,
            Self::ActivateEventPublisher { .. } => crate::application::access::ACTIVATE_PUBLISHER_OPERATION,
            Self::DeactivateEventPublisher { .. } => crate::application::access::DEACTIVATE_PUBLISHER_OPERATION,
            Self::DeleteEventPublisher { .. } => crate::application::access::DELETE_PUBLISHER_OPERATION,
        }
    }
}
