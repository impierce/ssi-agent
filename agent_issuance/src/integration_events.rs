use serde::{Deserialize, Serialize};
use shared_kernel::IntegrationEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IssuanceIntegrationEvent {
    CredentialOffered {
        offer_id: String,
        credential_ids: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pre_authorized_code: Option<String>,
    },
    CredentialIssued {
        credential_id: String,
        status: String,
    },
    CredentialRevoked {
        credential_id: String,
    },
}

impl IntegrationEvent for IssuanceIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::CredentialOffered { .. } => "tech.impierce.unicore.issuance.credential.offered",
            Self::CredentialIssued { .. } => "tech.impierce.unicore.issuance.credential.issued",
            Self::CredentialRevoked { .. } => "tech.impierce.unicore.issuance.credential.revoked",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::CredentialOffered { offer_id, .. } => Some(offer_id.clone()),
            Self::CredentialIssued { credential_id, .. } | Self::CredentialRevoked { credential_id } => {
                Some(credential_id.clone())
            }
        }
    }
}
