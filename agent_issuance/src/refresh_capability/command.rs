use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum RefreshCapabilityCommand {
    CreateRefreshCapability {
        refresh_reference: String,
        credential_id: String,
    },
    DisableRefreshCapability,
}
