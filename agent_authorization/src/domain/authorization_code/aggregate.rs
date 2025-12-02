use super::command::AuthorizationCodeCommand;
use super::error::AuthorizationCodeError;
use super::event::AuthorizationCodeEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vci::{authorization_request::CodeChallengeMethod, pkce};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthorizationCode {
    #[serde(rename = "id")]
    pub authorization_code_id: String,
    pub client_id: String,
    pub redirect_uri: Option<Url>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<CodeChallengeMethod>,
    pub issuer_state: Option<String>,
    pub expires_at: Option<i64>,
    pub is_redeemed: bool,
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

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationCodeCommand::*;
        use AuthorizationCodeError::*;
        use AuthorizationCodeEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationCode {
                authorization_code_id,
                client_id,
                redirect_uri,
                code_challenge,
                code_challenge_method,
                issuer_state,
                expires_in,
            } => {
                #[cfg(not(test))]
                let expires_at = chrono::Utc::now().timestamp() + expires_in;
                #[cfg(test)]
                let expires_at = expires_in;

                Ok(vec![AuthorizationCodeCreated {
                    authorization_code_id: authorization_code_id.clone(),
                    client_id,
                    redirect_uri,
                    code_challenge,
                    code_challenge_method,
                    issuer_state,
                    expires_at,
                }])
            }
            RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                code_verifier,
            } => {
                // Check if already used.
                if self.is_redeemed {
                    return Err(RedeemedAuthorizationCodeError);
                }

                // Check expiry.
                if let Some(expires_at) = self.expires_at {
                    #[cfg(not(test))]
                    let now = chrono::Utc::now().timestamp();
                    #[cfg(test)]
                    let now = 0;

                    if now > expires_at {
                        return Err(ExpiredAuthorizationCodeError);
                    }
                }

                // Validate Client ID.
                if self.client_id != client_id {
                    return Err(InvalidClientIdError);
                }

                // Validate redirect_uri.
                if self.redirect_uri != redirect_uri {
                    return Err(InvalidRedirectUriError);
                }

                // Validate PKCE code_verifier
                if let Some(code_challenge) = &self.code_challenge {
                    let code_verifier = code_verifier.ok_or(MissingCodeVerifierError)?;

                    if pkce::code_challenge(code_verifier.as_bytes()) != *code_challenge {
                        return Err(InvalidCodeVerifierError);
                    }
                }

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
                code_challenge,
                code_challenge_method,
                issuer_state,
                expires_at,
            } => {
                self.authorization_code_id = authorization_code_id;
                self.client_id = client_id;
                self.redirect_uri.replace(redirect_uri);
                self.code_challenge = code_challenge;
                self.code_challenge_method = code_challenge_method;
                self.issuer_state = issuer_state;
                self.expires_at.replace(expires_at);
            }
            AuthorizationCodeRedeemed {
                authorization_code_id,
                redeemed,
            } => {
                self.authorization_code_id = authorization_code_id;
                self.is_redeemed = redeemed;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub mod authorization_code_tests {
    use super::test_utils::*;
    use super::*;
    use crate::domain::oauth2_authorization_request::aggregate::test_utils::{
        code_challenge, code_challenge_method, code_verifier, redirect_uri,
    };
    use agent_authorization::domain::access_token::aggregate::test_utils::{client_id, issuer_state};
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type AccessTokenTestFramework = TestFramework<AuthorizationCode>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_authorization_code(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        redirect_uri: Option<Url>,
        issuer_state: Option<String>,
        expires_in: i64,
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given_no_previous_events()
            .when(AuthorizationCodeCommand::CreateAuthorizationCode {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge.clone()),
                code_challenge_method: code_challenge_method.clone(),
                issuer_state: issuer_state.clone(),
                expires_in,
            })
            .then_expect_events(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id,
                client_id,
                redirect_uri: redirect_uri.unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        code_verifier: &[u8],
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                code_verifier: Some(String::from_utf8(code_verifier.to_vec()).unwrap()),
            })
            .then_expect_events(vec![AuthorizationCodeEvent::AuthorizationCodeRedeemed {
                authorization_code_id,
                redeemed: true,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code_twice(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        code_verifier: &[u8],
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![
                AuthorizationCodeEvent::AuthorizationCodeCreated {
                    authorization_code_id: authorization_code_id.clone(),
                    client_id: client_id.clone(),
                    redirect_uri: redirect_uri.clone().unwrap(),
                    code_challenge: Some(code_challenge),
                    code_challenge_method,
                    issuer_state,
                    expires_at,
                },
                // The authorization code is already redeemed.
                AuthorizationCodeEvent::AuthorizationCodeRedeemed {
                    authorization_code_id,
                    redeemed: true,
                },
            ])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                code_verifier: Some(String::from_utf8(code_verifier.to_vec()).unwrap()),
            })
            .then_expect_error_message(&AuthorizationCodeError::RedeemedAuthorizationCodeError.to_string());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_expired_authorization_code(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        code_verifier: &[u8],
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                // The authorization code is immediately expired.
                expires_at: -1,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                code_verifier: Some(String::from_utf8(code_verifier.to_vec()).unwrap()),
            })
            .then_expect_error_message(&AuthorizationCodeError::ExpiredAuthorizationCodeError.to_string());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code_with_invalid_client_id(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        code_verifier: &[u8],
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id,
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                // Using an invalid client_id
                client_id: "invalid_client_id".to_string(),
                redirect_uri,
                code_verifier: Some(String::from_utf8(code_verifier.to_vec()).unwrap()),
            })
            .then_expect_error_message(&AuthorizationCodeError::InvalidClientIdError.to_string());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code_with_invalid_redirect_uri(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        code_verifier: &[u8],
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                // Using an invalid redirect_uri
                redirect_uri: Some(Url::parse("https://invalid-redirect-uri.test").unwrap()),
                code_verifier: Some(String::from_utf8(code_verifier.to_vec()).unwrap()),
            })
            .then_expect_error_message(&AuthorizationCodeError::InvalidRedirectUriError.to_string());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code_with_missing_code_verifier(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                // Missing code_verifier
                code_verifier: None,
            })
            .then_expect_error_message(&AuthorizationCodeError::MissingCodeVerifierError.to_string());
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_redeem_authorization_code_with_invalid_code_verifier(
        authorization_code_id: String,
        client_id: String,
        code_challenge: String,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        redirect_uri: Option<Url>,
        expires_at: i64,
    ) {
        AccessTokenTestFramework::with(())
            .given(vec![AuthorizationCodeEvent::AuthorizationCodeCreated {
                authorization_code_id: authorization_code_id.clone(),
                client_id: client_id.clone(),
                redirect_uri: redirect_uri.clone().unwrap(),
                code_challenge: Some(code_challenge),
                code_challenge_method,
                issuer_state,
                expires_at,
            }])
            .when(AuthorizationCodeCommand::RedeemAuthorizationCode {
                client_id,
                redirect_uri,
                // Using an invalid code_verifier
                code_verifier: Some("invalid_code_verifier".to_string()),
            })
            .then_expect_error_message(&AuthorizationCodeError::InvalidCodeVerifierError.to_string());
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use rstest::fixture;

    #[fixture]
    pub fn authorization_code_id() -> String {
        "authorization_code_id".to_string()
    }

    #[fixture]
    pub fn expires_in() -> i64 {
        600 // 10 minutes
    }

    #[fixture]
    pub fn expires_at(expires_in: i64) -> i64 {
        expires_in
    }
}
