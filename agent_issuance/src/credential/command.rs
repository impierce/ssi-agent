use oid4vci::{
    credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject,
    notification_request::NotificationRequest, proofs::Proofs,
};
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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
        status_list_id: String,
        index: usize,
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

impl CommandOperation for CredentialCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateUnsignedCredential { .. } => "issuance.credentials.unsigned.create",
            Self::CreateSignedCredential { .. } => "issuance.credentials.signed.create",
            Self::SignCredential { .. } => "issuance.credentials.sign",
            Self::AddNotification { .. } => "issuance.credentials.notifications.add",
            Self::UpdateCredentialStatus { .. } => "issuance.credentials.status.update",
        }
    }
}
