use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    notification_request::NotificationRequest, proofs::Proofs,
};
use serde::Deserialize;

use crate::credential::aggregate::CredentialStatus;

use super::{aggregate::CredentialExpiry, entity::Data};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    CreateUnsignedCredential {
        credential_id: String,
        data: Data,
        credential_configuration: Box<CredentialConfigurationsSupportedObject>,
        expires_at: CredentialExpiry,
        credential_status_index: usize,
    },
    CreateSignedCredential {
        credential_id: String,
        signed_credential: serde_json::Value,
    },
    SignCredential {
        credential_id: String,
        subject_id: Option<String>,
        // When true, a credential will be re-signed if it already exists.
        overwrite: bool,
        proofs: Option<Proofs>,
    },
    AddNotification {
        credential_id: String,
        notification: NotificationRequest,
    },
    UpdateCredentialStatus {
        credential_id: String,
        credential_status: CredentialStatus,
    },
}
