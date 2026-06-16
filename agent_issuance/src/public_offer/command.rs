use serde::{Deserialize, Serialize};

/// Commands that represent user intentions for public offers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PublicOfferCommand {
    Create { offer_id: String, template_id: String },
    TakeOffline { offer_id: String },
    TakeOnline { offer_id: String },
    Delete { offer_id: String },
}
