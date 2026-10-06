use serde::{Deserialize, Serialize};
use shared_kernel::IntegrationEvent;

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
            Self::InvitationCreated { .. } => "tech.impierce.unicore.connection.invitation.created",
            Self::ConnectionEstablished { .. } => "tech.impierce.unicore.connection.established",
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
