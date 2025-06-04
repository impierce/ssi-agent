use crate::domain::access_token;

use super::command::AccessTokenCommand;
use super::error::AccessTokenError;
use super::event::AccessTokenEvent;
use agent_shared::config::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct AccessToken {
    #[serde(rename = "id")]
    pub access_token_id: String,
    pub access_token_value: String, // FIXME: use JWT
    // FIXME: can all these be removed?
    pub user_id: String,
    pub client_id: String,
    pub scopes: Option<String>,
    pub access_token_expires_at: u64,
    pub refresh_token_expires_at: Option<u64>,
    pub issuer_state: Option<String>,
}

// Placeholder for JWT claims structure - define this properly
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,            // Issuer
    sub: String,            // Subject (user_id)
    aud: String,            // Audience (client_id or your resource server identifier)
    exp: i64,               // Expiration Time
    iat: i64,               // Issued At
    jti: String,            // JWT ID (unique identifier for this token)
    scopes: Option<String>, // Or Vec<String>
    client_id: String,
    issuer_state: Option<String>, // Custom state for issuer
}

#[async_trait]
impl Aggregate for AccessToken {
    type Command = AccessTokenCommand;
    type Event = AccessTokenEvent;
    type Error = AccessTokenError;
    type Services = ();

    fn aggregate_type() -> String {
        "access_token".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AccessTokenCommand::*;
        use AccessTokenError::*;
        use AccessTokenEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            IssueAccessToken {
                access_token_id,
                user_id,
                client_id,
                scopes,
                access_token_expires_in,
                refresh_token_expires_in,
                issuer_state,
            } => {
                let now = chrono::Utc::now();
                let iat = now.timestamp();

                let access_token_expires_at = chrono::Utc::now().timestamp() as u64 + access_token_expires_in;
                let refresh_token_expires_at =
                    refresh_token_expires_in.map(|duration| chrono::Utc::now().timestamp() as u64 + duration);

                // --- Access Token (JWT) ---
                let exp = access_token_expires_at as i64; // Expiration time in seconds
                let jti = self.access_token_id.clone(); // Use the access token ID as the JWT ID

                // TODO: These should come from configuration or services
                let iss = config().public_url.to_string(); // FIXME: use DID?
                let aud = client_id.clone(); // FIXME: Or a specific RS identifier | Or Credential Issuer?

                let claims = Claims {
                    iss,
                    sub: user_id.clone(),
                    aud,
                    exp,
                    iat,
                    jti,
                    scopes: scopes.clone(),
                    client_id: client_id.clone(),
                    issuer_state: issuer_state.clone(),
                };

                // FIXME: Key Management! This is a placeholder.
                // The key should be securely managed and ideally not hardcoded or directly in the aggregate.
                // For HS256, the key is a byte slice. For RS256, it's an RSA private key.
                // Consider using `services` to provide a signing capability.
                let encoding_key_str = "your-256-bit-secret"; // FIXME: NEVER hardcode keys in production! Load from secure config.
                let encoding_key = jsonwebtoken::EncodingKey::from_secret(encoding_key_str.as_bytes());

                let access_token_value = jsonwebtoken::encode(
                    &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
                    &claims,
                    &encoding_key,
                )
                .expect("FIX THIS");

                Ok(vec![AccessTokenIssued {
                    access_token_id: access_token_id.clone(),
                    access_token_value,
                    user_id,
                    client_id,
                    scopes,
                    access_token_expires_at,
                    refresh_token_expires_at,
                    issuer_state,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AccessTokenEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            AccessTokenIssued {
                access_token_id,
                access_token_value, // FIXME: Use JWT
                user_id,
                client_id,
                scopes,
                access_token_expires_at,
                refresh_token_expires_at,
                issuer_state,
            } => {
                self.access_token_id = access_token_id;
                self.access_token_value = access_token_value; // FIXME: Store JWT or its value
                self.user_id = user_id;
                self.client_id = client_id;
                self.scopes = scopes;
                self.access_token_expires_at = access_token_expires_at;
                self.refresh_token_expires_at = refresh_token_expires_at;
                self.issuer_state = issuer_state;
            }
        }
    }
}

#[cfg(test)]
pub mod token_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
