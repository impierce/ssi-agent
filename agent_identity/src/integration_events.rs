use crate::connection::event::ConnectionEvent;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::IntegrationEvent;

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConnectionIntegrationEvent {
    InvitationCreated {
        connection_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    ConnectionEstablished {
        connection_id: String,
    },
}

impl IntegrationEvent for ConnectionIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::InvitationCreated { .. } => "com.impierce.unicore.connection.invitation.created",
            Self::ConnectionEstablished { .. } => "com.impierce.unicore.connection.established",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::InvitationCreated { connection_id, .. } | Self::ConnectionEstablished { connection_id } => {
                Some(connection_id.clone())
            }
        }
    }
}

impl TryFrom<ConnectionEvent> for ConnectionIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: ConnectionEvent) -> Result<Self, Self::Error> {
        match event {
            ConnectionEvent::ConnectionAdded {
                connection_id, url, ..
            } => Ok(Self::InvitationCreated {
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

impl ConnectionIntegrationEvent {
    /// Translates an internal identity connection domain [`CloudEvent`] to a [`ConnectionIntegrationEvent`].
    #[must_use]
    pub fn project_from(domain_ce: &CloudEvent) -> Option<CloudEvent> {
        let data = domain_ce.data.as_ref()?;
        let caller_id = domain_ce.extension.callerid.clone();
        let caller_type = domain_ce.extension.callertype.clone();

        let integration_event = match domain_ce.event_type.as_str() {
            "com.impierce.unicore.connection-added" => {
                if let Ok(event) = serde_json::from_value::<ConnectionEvent>(serde_json::json!({
                    "ConnectionAdded": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let connection_id = data
                        .get("connection_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let url = data.get("url").and_then(|v| v.as_str()).map(ToString::to_string);

                    Self::InvitationCreated { connection_id, url }
                }
            }
            "com.impierce.unicore.connection-synced" => {
                if let Ok(event) = serde_json::from_value::<ConnectionEvent>(serde_json::json!({
                    "ConnectionSynced": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let connection_id = data
                        .get("connection_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();

                    Self::ConnectionEstablished { connection_id }
                }
            }
            "com.impierce.unicore.connection-changes-accepted" => {
                if let Ok(event) = serde_json::from_value::<ConnectionEvent>(serde_json::json!({
                    "ConnectionChangesAccepted": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let connection_id = data
                        .get("connection_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();

                    Self::ConnectionEstablished { connection_id }
                }
            }
            _ => return None,
        };

        let mut result_ce = integration_event
            .into_cloud_event("/services/connection", caller_id, caller_type)
            .ok()?;

        if let Some(occurred_at) = domain_ce.time {
            result_ce.time = Some(occurred_at);
        }

        Some(result_ce)
    }
}

/// Project an internal domain [`CloudEvent`] to a [`ConnectionIntegrationEvent`] if applicable.
#[must_use]
pub fn project_identity_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    ConnectionIntegrationEvent::project_from(domain_ce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::common::Url;

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
        let integration = ConnectionIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            ConnectionIntegrationEvent::InvitationCreated {
                connection_id: "conn-1".to_string(),
                url: Some("https://example.com/invite".to_string()),
            }
        );
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
