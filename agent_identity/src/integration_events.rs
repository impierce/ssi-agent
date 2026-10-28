use crate::connection::aggregate::Connection;
use crate::connection::event::ConnectionEvent;
use cqrs_es::EventEnvelope;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

/// Integration events emitted by the Connection / Identity bounded context (`/services/connection`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, IntegrationEvent)]
#[serde(tag = "type", content = "data")]
#[integration_event(source = "/services/connection")]
pub enum ConnectionIntegrationEvent {
    /// Invitation created for peer connection.
    #[serde(rename = "com.impierce.unicore.connection.invitation.created")]
    InvitationCreated {
        /// Unique identifier of the connection record.
        #[integration_event(subject)]
        connection_id: String,
        /// Optional endpoint URL of the peer connection.
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// Peer connection synced or accepted.
    #[serde(rename = "com.impierce.unicore.connection.established")]
    ConnectionEstablished {
        /// Unique identifier of the established connection record.
        #[integration_event(subject)]
        connection_id: String,
    },
}

impl TryFrom<ConnectionEvent> for ConnectionIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: ConnectionEvent) -> Result<Self, Self::Error> {
        match event {
            ConnectionEvent::ConnectionAdded { connection_id, url, .. } => Ok(Self::InvitationCreated {
                connection_id,
                url: Some(url.to_string()),
            }),
            ConnectionEvent::ConnectionSynced { connection_id, .. }
            | ConnectionEvent::ConnectionChangesAccepted { connection_id, .. } => {
                Ok(Self::ConnectionEstablished { connection_id })
            }
            _ => Err(UnmappedEvent),
        }
    }
}

impl ProjectIntegrationEvent for Connection {
    type IntegrationEvent = ConnectionIntegrationEvent;

    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        ConnectionIntegrationEvent::try_from(envelope.payload.clone())
            .ok()?
            .into_projected_cloud_event(envelope)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::common::Url;
    use std::collections::HashMap;

    #[test]
    fn try_from_connection_added() {
        let event = ConnectionEvent::ConnectionAdded {
            connection_id: "conn-1".to_string(),
            display: None,
            url: Url::parse("https://example.com/invite").unwrap(),
            dids: vec![],
            first_interacted_at: None,
            last_interacted_at: None,
            validations: vec![],
        };
        let integration = ConnectionIntegrationEvent::try_from(event.clone()).expect("should map");
        assert_eq!(
            integration,
            ConnectionIntegrationEvent::InvitationCreated {
                connection_id: "conn-1".to_string(),
                url: Some("https://example.com/invite".to_string()),
            }
        );

        let envelope = EventEnvelope {
            aggregate_id: "conn-1".to_string(),
            sequence: 1,
            payload: event,
            metadata: HashMap::default(),
        };
        let projected = Connection::project_integration_event(&envelope).expect("should project");
        assert_eq!(projected.id, "connection:conn-1:1");
        assert!(projected.time.is_none());
    }

    #[test]
    fn try_from_connection_synced() {
        let event = ConnectionEvent::ConnectionSynced {
            connection_id: "conn-2".to_string(),
            validations: vec![],
            pending_changes: None,
            last_interacted_at: None,
        };
        let integration = ConnectionIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            ConnectionIntegrationEvent::ConnectionEstablished {
                connection_id: "conn-2".to_string(),
            }
        );
    }
}
