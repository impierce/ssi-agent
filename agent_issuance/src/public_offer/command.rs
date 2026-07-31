use serde::{Deserialize, Serialize};
use shared_kernel::authorization::CommandOperation;

/// Commands that represent user intentions for public offers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PublicOfferCommand {
    Create { offer_id: String, template_id: String },
    TakeOffline { offer_id: String },
    TakeOnline { offer_id: String },
    Delete { offer_id: String },
}

impl CommandOperation for PublicOfferCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "issuance.public_offers.create",
            Self::TakeOffline { .. } => "issuance.public_offers.take_offline",
            Self::TakeOnline { .. } => "issuance.public_offers.take_online",
            Self::Delete { .. } => "issuance.public_offers.delete",
        }
    }
}
