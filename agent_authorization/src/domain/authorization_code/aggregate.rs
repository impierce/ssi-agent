use super::command::AuthorizationCodeCommand;
use super::error::AuthorizationCodeError;
use super::event::AuthorizationCodeEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct AuthorizationCode {
    #[serde(rename = "id")]
    pub authorization_code_id: String,
    pub client_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub issuer_state: Option<String>,
    pub expires_at: Option<i64>,
    pub redeemed: bool,
}

#[async_trait]
impl Aggregate for AuthorizationCode {
    type Command = AuthorizationCodeCommand;
    type Event = AuthorizationCodeEvent;
    type Error = AuthorizationCodeError;
    type Services = ();

    fn aggregate_type() -> String {
        "authorization_code".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationCodeCommand::*;
        use AuthorizationCodeError::*;
        use AuthorizationCodeEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationCode {
                authorization_code_id,
                client_id,
                user_id,
                redirect_uri,
                scope,
                code_challenge,
                code_challenge_method,
                issuer_state,
                // FIXME: should not be optional
                expires_in,
            } => {
                // FIXME: i64 vs u64
                let expires_at = expires_in.map(|duration| chrono::Utc::now().timestamp() + duration as i64);

                Ok(vec![AuthorizationCodeCreated {
                    authorization_code_id: authorization_code_id.clone(),
                    client_id,
                    redirect_uri,
                    scope,
                    user_id,
                    code_challenge,
                    code_challenge_method,
                    issuer_state,
                    expires_at,
                }])
            }
            RedeemCode {
                client_id,
                redirect_uri,
                code_verifier,
            } => {
                // // 1. Check if already initialized/created
                // if !self.initialized {
                //     // or self.code.is_none()
                //     return Err(InvalidGrant("Code does not exist or not initialized".to_string()));
                // }

                // // 2. Check if already used
                // if self.used.unwrap_or(false) {
                //     // Assuming `used` is Option<bool> or bool
                //     return Err(InvalidGrant("Authorization code has already been used".to_string()));
                // }

                // // 3. Check expiry
                // if let Some(expires_at_ts) = self.expires_at {
                //     if chrono::Utc::now().timestamp() > expires_at_ts {
                //         // Optionally emit an AuthorizationCodeExpired event here too, or just error
                //         return Err(InvalidGrant("Authorization code has expired".to_string()));
                //     }
                // } else {
                //     // Should not happen if expires_in was set during creation
                //     return Err(GenericError("Code expiry not set".to_string()));
                // }

                // 4. Validate client_id
                if self.client_id != client_id {
                    todo!()
                    // return Err(InvalidGrant("Client ID mismatch".to_string()));
                }

                // 5. Validate redirect_uri (if applicable)
                //    The spec says if redirect_uri was present in the initial auth request,
                //    it MUST be present in the token request and be an exact match.
                if &self.redirect_uri != redirect_uri.as_ref().unwrap() {
                    todo!()
                    // return Err(InvalidGrant("Redirect URI mismatch".to_string()));
                }

                // // 6. Validate PKCE code_verifier
                // if let Some(challenge) = &self.code_challenge {
                //     todo!()
                //     // let verifier = code_verifier.ok_or_else(|| InvalidGrant("Code verifier required".to_string()))?;
                //     // // Implement PKCE verification logic here based on self.code_challenge_method
                //     // // For S256: hash = SHA256(verifier_ascii); encoded_challenge = BASE64URL-NOPAD(hash)
                //     // // For plain: challenge == verifier
                //     // if !verify_pkce_challenge(&verifier, challenge, self.code_challenge_method.as_deref()) {
                //     //     return Err(InvalidGrant("Invalid PKCE code verifier".to_string()));
                //     // }
                // } else if code_verifier.is_some() {
                //     // Code challenge was not set, but verifier was provided - this might be an error or ignored
                //     // depending on policy. Generally, if no challenge, no verifier should be expected.
                // }

                // All checks passed, emit an event to mark as used and carry forward necessary data
                Ok(vec![AuthorizationCodeRedeemed {
                    authorization_code_id: self.authorization_code_id.clone(),
                    redeemed: true,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationCodeEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            AuthorizationCodeCreated {
                authorization_code_id,
                client_id,
                redirect_uri,
                scope,
                user_id,
                code_challenge,
                code_challenge_method,
                issuer_state,
                expires_at,
            } => {
                // Here you would implement the logic to apply the event to the aggregate state
                // For now, we just set the authorization_code_id
                self.authorization_code_id = authorization_code_id;
                self.client_id = client_id;
                self.redirect_uri = redirect_uri;
                self.scope = scope;
                self.user_id = user_id;
                self.code_challenge = code_challenge;
                self.code_challenge_method = code_challenge_method;
                self.issuer_state = issuer_state;
                self.expires_at = expires_at;
            }
            AuthorizationCodeRedeemed {
                authorization_code_id: _,
                redeemed,
            } => {
                // Mark the code as redeemed
                self.redeemed = redeemed;
                // FIXME: comment below does probably not really make sense since we store all events.
                // Optionally, you might want to clear sensitive data or set a used flag
                // self.code_challenge = None; // Clear challenge if you don't want to keep it after use
                // self.code_challenge_method = None; // Clear method if not needed anymore
            }
        }
    }
}

// FIXME: Use a proper PKCE library or implement your own
// // Dummy PKCE verification function - replace with a proper one from a crypto library or your own impl
// fn verify_pkce_challenge(verifier: &str, challenge: &str, method: Option<&str>) -> bool {
//     match method {
//         Some("S256") | None => {
//             // Default to S256 if method is None but challenge is present
//             use base64_url;
//             use sha2::{Digest, Sha256};

//             let mut hasher = Sha256::new();
//             hasher.update(verifier.as_bytes());
//             let hashed_verifier = hasher.finalize();
//             let encoded_hashed_verifier = base64_url::encode(&hashed_verifier);
//             encoded_hashed_verifier == challenge
//         }
//         Some("plain") => verifier == challenge,
//         Some_ => false, // Unknown method
//     }
// }

#[cfg(test)]
pub mod authorization_code_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
