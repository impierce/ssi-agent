use serde::{Deserialize, Serialize};

// TODO: cleanup unnused attributes, not only here
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NonceCommand {
    GenerateNonce { c_nonce: String },
    RedeemNonce { c_nonce: String },
}
