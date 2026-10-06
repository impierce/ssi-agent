use serde::{Deserialize, Serialize};
use shared_kernel::IntegrationEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VerificationIntegrationEvent {
    PresentationRequested {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
    },
    PresentationVerified {
        request_id: String,
        validated: bool,
    },
    PresentationFailed {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl IntegrationEvent for VerificationIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::PresentationRequested { .. } => "tech.impierce.unicore.verification.presentation.requested",
            Self::PresentationVerified { .. } => "tech.impierce.unicore.verification.presentation.verified",
            Self::PresentationFailed { .. } => "tech.impierce.unicore.verification.presentation.failed",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::PresentationRequested { request_id, .. }
            | Self::PresentationVerified { request_id, .. }
            | Self::PresentationFailed { request_id, .. } => Some(request_id.clone()),
        }
    }
}
