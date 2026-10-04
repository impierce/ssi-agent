use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum RefreshCapabilityCommand {
    CreateRefreshCapability {
        refresh_reference: String,
        credential_id: String,
    },
    DisableRefreshCapability,
}

impl shared_kernel::authorization::CommandOperation for RefreshCapabilityCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateRefreshCapability { .. } => "issuance.refresh_capabilities.create",
            Self::DisableRefreshCapability => "issuance.refresh_capabilities.disable",
        }
    }
}
