use crate::nonce::command::NonceCommand;
use crate::state::IssuanceState;
use agent_shared::handlers::{command_handler, query_handler};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use oid4vci::credential_request::CredentialRequest;
use oid4vci::proof::Proof;
use thiserror::Error;

pub struct NonceValidationService;

#[derive(Debug, Error)]
pub enum NonceValidationError {
    #[error("Invalid nonce: either it does not exist or has already been redeemed")]
    InvalidNonce,
    #[error("Missing nonce in the proof")]
    MissingNonce,
    #[error("Redeemed nonce: the nonce has already been used")]
    RedeemedNonce,
}

fn extract_nonce_from_proof(proof: &Proof) -> Option<String> {
    match proof {
        Proof::Jwt { jwt } => {
            let parts: Vec<&str> = jwt.split('.').collect();
            if parts.len() != 3 {
                return None;
            }

            let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
            let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

            // Extract nonce from claims
            claims.get("nonce").and_then(|n| n.as_str()).map(|s| s.to_string())
        }
    }
}

impl NonceValidationService {
    /// Validates a nonce included in the credential request.
    ///
    /// This function checks if the nonce exists and whether it has already been redeemed.
    pub async fn validate(
        state: &IssuanceState,
        credential_request: &CredentialRequest,
    ) -> Result<(), NonceValidationError> {
        if let Some(proof) = &credential_request.proof {
            if let Some(nonce) = extract_nonce_from_proof(proof) {
                // Query nonce state
                let nonce_status = query_handler(&nonce, &state.query.nonce)
                    .await
                    .map_err(|_| NonceValidationError::InvalidNonce)?;

                match nonce_status {
                    Some(n) if n.is_redeemed => {
                        // Nonce has already been redeemed
                        return Err(NonceValidationError::RedeemedNonce);
                    }
                    Some(_) => {
                        // Nonce is valid, redeem it
                        let command = NonceCommand::RedeemNonce { c_nonce: nonce.clone() };
                        command_handler(&nonce, &state.command.nonce, command)
                            .await
                            .map_err(|_| NonceValidationError::InvalidNonce)?;
                        return Ok(());
                    }
                    None => {
                        // Nonce doesn't exist
                        return Err(NonceValidationError::MissingNonce);
                    }
                }
            }
        }
        Ok(())
    }
}
