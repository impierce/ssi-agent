use crate::domain::event_publisher::{
    aggregate::EventPublisher, config::EventPublisherConfig, event::EventPublisherEvent,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared_kernel::{
    cqrs_es::{EventEnvelope, View},
    strum,
    view_repository::SoftDeletable,
};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum Status {
    Configured,
    #[default]
    None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct EventPublisherView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub events: Vec<String>,
    pub config: Option<EventPublisherConfig>,
    pub view_status: Status,
    pub created_at: DateTime<Utc>,
    pub last_modified_at: DateTime<Utc>,
}

impl EventPublisherView {
    /// Returns a copy of this view with sensitive fields (such as HTTP webhook header values) redacted.
    #[must_use]
    pub fn redacted(mut self) -> Self {
        self.config = self.config.map(|config| config.redacted());
        self
    }
}

impl View<EventPublisher> for EventPublisherView {
    fn update(&mut self, event: &EventEnvelope<EventPublisher>) {
        match &event.payload {
            EventPublisherEvent::EventPublisherCreated {
                id,
                name,
                description,
                enabled,
                events,
                config,
                occurred_at,
            } => {
                self.id.clone_from(id);
                self.name.clone_from(name);
                self.description.clone_from(description);
                self.enabled = *enabled;
                self.events.clone_from(events);
                self.config = Some(config.clone());
                self.view_status = Status::Configured;
                self.created_at = *occurred_at;
                self.last_modified_at = *occurred_at;
            }
            EventPublisherEvent::EventPublisherUpdated {
                id: _,
                name,
                description,
                enabled,
                events,
                config,
                occurred_at,
            } => {
                self.name.clone_from(name);
                self.description.clone_from(description);
                self.enabled = *enabled;
                self.events.clone_from(events);
                self.config = Some(config.clone());
                self.last_modified_at = *occurred_at;
            }
            EventPublisherEvent::EventPublisherActivated { occurred_at, .. } => {
                self.enabled = true;
                self.last_modified_at = *occurred_at;
            }
            EventPublisherEvent::EventPublisherDeactivated { occurred_at, .. } => {
                self.enabled = false;
                self.last_modified_at = *occurred_at;
            }
            EventPublisherEvent::EventPublisherDeleted { occurred_at, .. } => {
                self.enabled = false;
                self.view_status = Status::None;
                self.last_modified_at = *occurred_at;
            }
        }
    }
}

impl SoftDeletable for EventPublisherView {
    fn is_deleted(&self) -> bool {
        matches!(self.view_status, Status::None)
    }
}
