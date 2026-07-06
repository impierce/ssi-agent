use crate::credential::aggregate::CredentialStatus;

use super::{aggregate::Status, entity::Data};
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    notification_request::NotificationRequest,
};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum CredentialEvent {
    // TODO: rename to `DataCredentialCreated`?
    UnsignedCredentialCreated {
        credential_id: String,
        data: Data,
        notification_id: Option<String>,
        credential_configuration: Box<CredentialConfigurationsSupportedObject>,
        created_at: Option<DateTime<Utc>>,
        expires_at: Option<DateTime<Utc>>,
    },
    SignedCredentialCreated {
        credential_id: String,
        signed_credential: serde_json::Value,
        notification_id: Option<String>,
    },
    CredentialSigned {
        credential_id: String,
        signed_credential: serde_json::Value,
        credential_status: CredentialStatus,
        status: Status,
    },
    NotificationReceived {
        credential_id: String,
        notification: NotificationRequest,
    },
    CredentialStatusUpdated {
        credential_id: String,
        credential_status: CredentialStatus,
    },
}

impl DomainEvent for CredentialEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
