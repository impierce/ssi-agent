use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::authorization_request::AuthorizationRequest;
use oid4vci::wallet::PushedAuthorizationResponse;
use thiserror::Error;

use crate::{
    domain::oauth2_authorization_request::command::OAuth2AuthorizationRequestCommand,
    state::{AuthorizationState, UNIME_CLIENT_ID},
};

// TODO: improve error handling
#[derive(Debug, Error)]
pub enum PushedAuthorizationError {
    #[error("Invalid client ID")]
    InvalidClientIdError,
    #[error("Invalid redirect URI")]
    InvalidRedirectUriError,
    #[error("Invalid response type")]
    InvalidResponseTypeError,
    #[error("Missing code challenge")]
    MissingCodeChallengeError,
    #[error("Invalid code challenge method")]
    InvalidCodeChallengeMethodError,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct PushedAuthorizationService {}

impl PushedAuthorizationService {
    pub async fn handle_pushed_authorization_request(
        state: &AuthorizationState,
        pushed_authorization_request: AuthorizationRequest,
    ) -> Result<PushedAuthorizationResponse, PushedAuthorizationError> {
        // TODO: Currently there is no way of validating these parameters for unknown Clients (Clients that are not
        // registered in the Authorization Server). Therefore, as of now we can only validate the request for known
        // Clients. See: https://github.com/openid/OpenID4VCI/issues/94
        if pushed_authorization_request.client_id == UNIME_CLIENT_ID {
            let client = query_handler(&pushed_authorization_request.client_id, &state.query.client)
                .await
                .map_err(|err| PushedAuthorizationError::Internal(err.to_string()))?
                .ok_or(PushedAuthorizationError::InvalidClientIdError)?;

            pushed_authorization_request
                .redirect_uri
                .as_ref()
                .filter(|redirect_uri| client.redirect_uris.contains(redirect_uri))
                .ok_or(PushedAuthorizationError::InvalidRedirectUriError)?;

            if !client
                .response_types
                .contains(&pushed_authorization_request.response_type)
            {
                return Err(PushedAuthorizationError::InvalidResponseTypeError);
            }

            // todo: add scope validation

            if client.require_pkce {
                if let Some(code_challenge_method) = pushed_authorization_request.code_challenge_method.as_ref() {
                    if !client.code_challenge_methods_supported.contains(code_challenge_method) {
                        return Err(PushedAuthorizationError::InvalidCodeChallengeMethodError);
                    }
                } else {
                    return Err(PushedAuthorizationError::MissingCodeChallengeError);
                }

                // todo: add code_challenge validation
            }
        }

        let oauth2_authorization_request_id = uuid::Uuid::new_v4().urn().to_string();
        let request_uri = oauth2_authorization_request_id.clone();

        // TODO: Make this configurable?
        let expires_in = 3600; // 1 hour
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let command = OAuth2AuthorizationRequestCommand::CreateOAuth2AuthorizationRequest {
            oauth2_authorization_request_id: oauth2_authorization_request_id.clone(),
            pushed_authorization_request: pushed_authorization_request.clone(),
            expires_at,
            interaction_type: None,
        };

        command_handler(
            &oauth2_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .map_err(|err| PushedAuthorizationError::Internal(err.to_string()))?;

        Ok(PushedAuthorizationResponse {
            request_uri,
            expires_in,
        })
    }
}
