use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ReissuanceCommand {
    CreateReissuance {
        reissuance_id: String,
        original_credential_id: String,
        new_credential_id: String,
        offer_id: String,
        credential_configuration_id: String,
        reason: Option<String>,
        trigger_type: Option<String>,
        triggered_by: Option<String>,
        status_action: Option<String>,
    },
}
