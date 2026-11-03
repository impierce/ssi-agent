pub mod apply;

use crate::domain::event_publisher::config::{EventPublisherConfig, EventPublisherFilter};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared_kernel::{
    convert_case::{Case, Casing},
    cqrs_es::DomainEvent,
    strum,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, strum::Display)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EventPublisherEvent {
    EventPublisherCreated {
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    },
    EventPublisherUpdated {
        id: String,
        name: String,
        description: Option<String>,
        enabled: bool,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    },
    EventPublisherActivated {
        id: String,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    },
    EventPublisherDeactivated {
        id: String,
        occurred_at: DateTime<Utc>,
    },
    EventPublisherDeleted {
        id: String,
        occurred_at: DateTime<Utc>,
    },
}

impl DomainEvent for EventPublisherEvent {
    fn event_type(&self) -> String {
        self.to_string().to_case(Case::Kebab)
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
