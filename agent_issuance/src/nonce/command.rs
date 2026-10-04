use serde::{Deserialize, Serialize};
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NonceCommand {
    GenerateNonce { c_nonce: String },
    RedeemNonce { c_nonce: String },
}

impl CommandOperation for NonceCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::GenerateNonce { .. } => "issuance.nonces.generate",
            Self::RedeemNonce { .. } => "issuance.nonces.redeem",
        }
    }
}
