use crate::state::IssuanceState;
use agent_shared::config::config;
use identity_core::convert::{FromJson as _, ToJson as _};
use jsonwebtoken::{decode, jwk::Jwk as JsonWebTokenJwk, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use thiserror::Error;

pub struct AccessTokenValidationService;

#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize)]
pub struct AccessTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub jti: String,
    // TODO: implement `scope`
    pub scope: Option<String>,
    pub client_id: String,
    pub issuer_state: Option<String>,
}

// TODO: define more specific errors for access token validation.
/// Defines the possible errors that can occur during access token validation.
#[derive(Debug, Error)]
pub enum AccessTokenValidationError {
    #[error("Token is malformed or its signature is invalid")]
    InvalidToken,
    #[error("Failed to resolve the public key for the token's `kid`")]
    KidResolutionError,
    
}

impl AccessTokenValidationService {
    /// Validates a JWT access token.
    ///
    /// This function checks the token's signature, expiration, issuer and audience.
    pub async fn validate(
        state: &IssuanceState,
        access_token: &str,
    ) -> Result<AccessTokenClaims, AccessTokenValidationError> {
        let jwt = jsonwebtoken::decode_header(access_token).map_err(|_| AccessTokenValidationError::InvalidToken)?;
        let algorithm = jwt.alg;
        let kid = jwt.kid.as_ref().ok_or(AccessTokenValidationError::InvalidToken)?;

        // In a real system, you would resolve the key based on the JWT's `kid` header.
        // For now, we assume a default key known to the issuer.
        let public_key_jwk = state
            .subject
            .resolve_public_key(kid)
            .await
            .map_err(|_err| AccessTokenValidationError::KidResolutionError)?;

        // Convert the `IotaIdentityJwk` first into a `JsonWebTokenJwk` and then into a `DecodingKey`.
        let decoding_key = public_key_jwk
            .to_json()
            .ok()
            .and_then(|public_key| JsonWebTokenJwk::from_json(&public_key).ok())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
            .ok_or(AccessTokenValidationError::KidResolutionError)?;

        let public_url = config().public_url.to_string();

        // Setup validation rules. The audience and issuer are the URL of this server.
        let mut validation = Validation::new(algorithm);

        // TODO: Could/should this be DIDs?
        // The audience is the public URL of this server.
        validation.set_audience(&[public_url.as_str()]);

        // TODO: allow external Authorization Servers
        // The issuer is the public URL of this server.
        validation.set_issuer(&[public_url.as_str()]);

        // Decode the token. This checks the signature and standard time-based claims (`exp`).
        let token_data = decode::<AccessTokenClaims>(access_token, &decoding_key, &validation)
            .map_err(|_| AccessTokenValidationError::InvalidToken)?;

        Ok(token_data.claims)
    }
}
