use crate::credential::aggregate::CredentialStatus;

use super::{aggregate::Status, entity::Data};
use crate::server_config::supported_configuration::SupportedCredentialConfiguration;
use chrono::{DateTime, Utc};
use cqrs_es::DomainEvent;
use oid4vci::notification_request::NotificationRequest;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum CredentialEvent {
    // TODO: rename to `DataCredentialCreated`?
    UnsignedCredentialCreated {
        credential_id: String,
        data: Data,
        notification_id: Option<String>,
        credential_configuration: Box<SupportedCredentialConfiguration>,
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

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
