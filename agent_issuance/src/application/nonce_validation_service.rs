use crate::nonce::command::NonceCommand;
use crate::state::IssuanceState;
use agent_shared::handlers::{command_handler, query_handler};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use oid4vci::credential_request::CredentialRequest;
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
        let nonces = extract_nonce_from_credential_request(credential_request);

        if nonces.is_empty() {
            Err(NonceValidationError::MissingNonce);
        }

        // All the c_nonces within the proofs of a singular CredentialRequest should be the same.
        if !nonces.iter().all(|n| n == &nonces[0]) {
            Err(NonceValidationError::InvalidNonce);
        }

        let nonce = &nonces[0];

        let nonce_status = query_handler(nonce, &state.query.nonce)
            .await
            .map_err(|_| NonceValidationError::InvalidNonce)?;

        match nonce_status {
            Some(n) if n.is_redeemed => Err(NonceValidationError::RedeemedNonce),
            Some(_) => {
                let command = NonceCommand::RedeemNonce { c_nonce: nonce.clone() };
                command_handler(nonce, &state.command.nonce, command)
                    .await
                    .map_err(|_| NonceValidationError::InvalidNonce)?;
                Ok(())
            }
            None => Err(NonceValidationError::MissingNonce),
        }
    }
}

// Helpers
pub fn extract_nonce_from_credential_request(credential_request: &CredentialRequest) -> Vec<String> {
    let Some(proofs) = &credential_request.proofs else {
        return vec![];
    };

    proofs
        .jwt
        .iter()
        .filter_map(|jwt: &String| {
            let parts: Vec<&str> = jwt.split('.').collect();
            if parts.len() != 3 {
                return None;
            }

            let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
            let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

            // Extract nonce from claims
            claims.get("nonce").and_then(|n| n.as_str()).map(|s| s.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use agent_issuance::application::nonce_validation_service::{
        extract_nonce_from_credential_request, NonceValidationError, NonceValidationService,
    };
    use agent_issuance::nonce::command::NonceCommand;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_shared::handlers::command_handler;
    use agent_store::in_memory::InMemory;
    use oid4vci::proofs::Proofs;

    use agent_store::issuance_state;
    use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId;
    use oid4vci::credential_request::CredentialRequest;

    use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId;
    use rstest::{fixture, rstest};
    use std::sync::Arc;

    const PROOF_JWT: &str = "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBYtgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ";
    const NONCE_VALUE: &str = "7e03ad3f76cb3338c3a5642fe7634476aa3ad93fa1d584011ba2150d9da47133";
    const NONCE_VALUE_2: &str = "8e03ad3f76cb3338c3a5642fe7634476aa3ad93fa1d584011ba2150d9da47133";

    #[test]
    fn test_extract_nonce_from_credential_request() {
        let credential_request = CredentialRequest {
            credential_identifier_or_credential_configuration_id:
                CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId(
                    "test.credential".to_string(),
                ),
            proofs: Some(Proofs {
                jwt: vec![PROOF_JWT.to_string()],
            }),
        };

        let nonces = extract_nonce_from_credential_request(&credential_request);
        assert_eq!(nonces, vec![NONCE_VALUE.to_string()]);
    }

    #[test]
    fn test_extract_nonce_from_malformed_jwt() {
        let credential_request = CredentialRequest {
            credential_identifier_or_credential_configuration_id:
                CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId(
                    "test.credential".to_string(),
                ),
            proofs: Some(Proofs {
                jwt: vec!["malformed.jwt".to_string()],
            }),
        };

        let nonces = extract_nonce_from_credential_request(&credential_request);
        assert!(nonces.is_empty());
    }

    #[rstest]
    async fn test_valid_nonce_successful_validation(credential_request: CredentialRequest) {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();

        let nonce = NONCE_VALUE.to_string();

        let create_command = NonceCommand::GenerateNonce { c_nonce: nonce.clone() };
        command_handler(NONCE_VALUE, &state.command.nonce, create_command)
            .await
            .unwrap();

        let result = NonceValidationService::validate(&state, &credential_request).await;
        assert!(result.is_ok());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_validate_redeemed_nonce_fails(credential_request: CredentialRequest) {
        let nonce = NONCE_VALUE.to_string();
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();

        let create_command = NonceCommand::GenerateNonce { c_nonce: nonce.clone() };
        command_handler(NONCE_VALUE, &state.command.nonce, create_command)
            .await
            .unwrap();

        let redeem_command = NonceCommand::RedeemNonce { c_nonce: nonce.clone() };
        command_handler(NONCE_VALUE, &state.command.nonce, redeem_command)
            .await
            .unwrap();

        let result = NonceValidationService::validate(&state, &credential_request).await;
        assert!(matches!(result, Err(NonceValidationError::RedeemedNonce)));
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_validate_wrong_nonce(credential_request: CredentialRequest) {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();

        let create_command = NonceCommand::GenerateNonce {
            c_nonce: NONCE_VALUE_2.to_string(),
        };
        command_handler(NONCE_VALUE_2, &state.command.nonce, create_command)
            .await
            .unwrap();

        let result = NonceValidationService::validate(&state, &credential_request).await;
        assert!(matches!(result, Err(NonceValidationError::MissingNonce)));
    }

    #[fixture]
    fn credential_request() -> CredentialRequest {
        CredentialRequest {
            credential_identifier_or_credential_configuration_id: CredentialConfigurationId("001".to_string()),
            proofs: Some(Proofs {
                jwt: vec![PROOF_JWT.to_string()],
            }),
        }
    }
}
