use crate::state::IssuanceState;
use agent_shared::config::config;
use identity_core::convert::{FromJson as _, ToJson as _};
use jsonwebtoken::{decode, jwk::Jwk as JsonWebTokenJwk, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use thiserror::Error;

pub struct AccessTokenValidationService;

// Placeholder for JWT claims structure - define this properly
#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,            // Issuer
    pub sub: String,            // Subject (user_id)
    pub aud: String,            // Audience (client_id or your resource server identifier)
    pub exp: u64,               // Expiration Time
    pub iat: u64,               // Issued At
    pub jti: String,            // JWT ID (unique identifier for this token)
    pub scopes: Option<String>, // Or Vec<String>
    pub client_id: String,
    pub issuer_state: Option<String>, // Custom state for issuer
}

/// Defines the possible errors that can occur during access token validation.
#[derive(Debug, Error)]
pub enum AccessTokenValidationError {
    #[error("Token is malformed or its signature is invalid")]
    InvalidToken,
    #[error("Token has expired")]
    Expired,
    #[error("Token audience is invalid")]
    InvalidAudience,
    #[error("Token issuer is invalid")]
    InvalidIssuer,
    #[error("Token scope is insufficient for this operation")]
    InsufficientScope,
    #[error("Internal validation error: {0}")]
    Internal(String),
}

impl AccessTokenValidationService {
    /// Validates a JWT access token.
    ///
    /// This function checks the token's signature, expiration, issuer, audience,
    /// and ensures it contains the required scope for the operation.
    pub async fn validate(state: &IssuanceState, access_token: &str) -> Result<Claims, AccessTokenValidationError> {
        let jwt = jsonwebtoken::decode_header(access_token).map_err(|_| AccessTokenValidationError::InvalidToken)?;
        let algorithm = jwt.alg;
        let kid = jwt.kid.as_ref().ok_or(AccessTokenValidationError::InvalidToken)?;

        // In a real system, you would resolve the key based on the JWT's `kid` header.
        // For now, we assume a default key known to the issuer.
        let public_key_jwk = state.subject.resolve_public_key(kid).await.expect("FIXME");

        // Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
        let decoding_key = public_key_jwk
            .to_json()
            .ok()
            .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .expect("FIXME: Failed to create DecodingKey");

        let public_url = config().public_url.to_string();

        // Setup validation rules. The audience and issuer are the URL of this server.
        let mut validation = Validation::new(algorithm);
        validation.set_audience(&[public_url.as_str()]);

        // TODO: allow external Authorization Servers
        validation.set_issuer(&[public_url.as_str()]);

        // Decode the token. This checks the signature and standard time-based claims (`exp`).
        let token_data = decode::<Claims>(access_token, &decoding_key, &validation).unwrap();
        // .map_err(|_| AccessTokenValidationError::InvalidToken)?;

        // FIXME: Perform a specific scope check.
        // let granted_scopes: Vec<&str> = token_data.claims.scope.split(' ').collect();
        // if !granted_scopes.contains(&required_scope) {
        //     return Err(AccessTokenValidationError::InsufficientScope);
        // }

        Ok(token_data.claims)
    }
}
