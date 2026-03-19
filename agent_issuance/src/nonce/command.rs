use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NonceCommand {
    GenerateNonce { c_nonce: String },
    RedeemNonce { c_nonce: String },
}
