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
    #[error("The nonce is malformed or could not be processed")]
    InvalidNonce,
    #[error("Required nonce could not be found")]
    MissingNonce,
    #[error("Nonce has already been redeemed")]
    RedeemedNonce,
}

impl NonceValidationService {
    /// Validates a nonce included in the credential request.
    ///
    /// This function checks if the nonce exists and whether it has already been redeemed.
    pub async fn validate(
        state: &IssuanceState,
        credential_request: &CredentialRequest,
    ) -> Result<(), NonceValidationError> {
        if let Some(nonce) = extract_nonce_from_credential(credential_request) {
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
        } else {
            Err(NonceValidationError::InvalidNonce)
        }
    }
}

// Helpers
fn extract_nonce_from_credential(credential_request: &CredentialRequest) -> Option<String> {
    let proof = credential_request.proof.as_ref()?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(test)]
    mod tests {
        use super::*;
        use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId;

        #[test]
        fn test_extract_nonce_from_proof() {
            const PROOF_JWT: &str = "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBYtgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ";
            const NONCE_VALUE: &str = "7e03ad3f76cb3338c3a5642fe7634476aa3ad93fa1d584011ba2150d9da47133";

            let credential_request = CredentialRequest {
                credential_identifier_or_credential_configuration_id:
                    CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId(
                        "test.credential".to_string(),
                    ),
                proof: Some(Proof::Jwt {
                    jwt: PROOF_JWT.to_string(),
                }),
                proofs: None,
            };

            let nonce = extract_nonce_from_credential(&credential_request);
            assert_eq!(nonce, Some(NONCE_VALUE.to_string()));
        }

        #[test]
        fn test_extract_nonce_from_malformed_jwt() {
            let credential_request = CredentialRequest {
                credential_identifier_or_credential_configuration_id:
                    CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId(
                        "test.credential".to_string(),
                    ),
                proof: Some(Proof::Jwt {
                    jwt: "malformed.jwt".to_string(),
                }),
                proofs: None,
            };

            let nonce = extract_nonce_from_credential(&credential_request);
            assert_
        }
    }
}
