use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::{
    InteractionType, InteractiveAuthorizationRequest, InteractiveAuthorizationResponse, InteractiveAuthorizationStatus,
};
use thiserror::Error;

use crate::{
    domain::{
        authorization_code::command::AuthorizationCodeCommand,
        oauth2_authorization_request::command::OAuth2AuthorizationRequestCommand,
    },
    state::{AuthorizationState, UNIME_CLIENT_ID},
};

/// This is the only interaction type that is currently supported by the Interactive Authorization Service. Will also stay the only type for the foreseeable future since it's the only one well-defined by OID4VCI 1.1 spec https://openid.github.io/OpenID4VCI/openid-4-verifiable-credential-issuance-1_1-wg-draft.html
pub const INTERACTION_TYPE_OPENID4VP: &str = "urn:openid:dcp:iae:openid4vp_presentation";

// TODO: improve error handling
#[derive(Debug, Error)]
pub enum InteractiveAuthorizationError {
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
    #[error("Request not found or expired")]
    RequestNotFound,
    #[error("Expired authorization request")]
    ExpiredAuthorizationRequestError,
    #[error("Missing redirect URI")]
    MissingRedirectUriError,
    #[error("Missing `openid4vp_response` in the request")]
    MissingOpenId4VPResponseError,
    #[error("Unsupported interaction types: {0}")]
    UnsupportedInteractionTypesError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct InteractiveAuthorizationService {}

impl InteractiveAuthorizationService {
    pub async fn handle_interactive_authorization_request(
        state: &AuthorizationState,
        interactive_authorization_request: InteractiveAuthorizationRequest,
    ) -> Result<InteractiveAuthorizationResponse, InteractiveAuthorizationError> {
        let InteractiveAuthorizationRequest {
            authorization_request,
            interaction_types_supported,
        } = interactive_authorization_request;

        if interaction_types_supported != INTERACTION_TYPE_OPENID4VP {
            return Err(InteractiveAuthorizationError::UnsupportedInteractionTypesError(
                interaction_types_supported,
            ));
        }

        // TODO: Currently there is no way of validating these parameters for unknown Clients (Clients that are not
        // registered in the Authorization Server). Therefore, as of now we can only validate the request for known
        // Clients. See: https://github.com/openid/OpenID4VCI/issues/94
        if authorization_request.client_id == UNIME_CLIENT_ID {
            let client = query_handler(&authorization_request.client_id, &state.query.client)
                .await
                .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?
                .ok_or(InteractiveAuthorizationError::InvalidClientIdError)?;

            authorization_request
                .redirect_uri
                .as_ref()
                .filter(|redirect_uri| client.redirect_uris.contains(redirect_uri))
                .ok_or(InteractiveAuthorizationError::InvalidRedirectUriError)?;

            if !client.response_types.contains(&authorization_request.response_type) {
                return Err(InteractiveAuthorizationError::InvalidResponseTypeError);
            }

            // todo: add scope validation

            if client.require_pkce {
                if let Some(code_challenge_method) = authorization_request.code_challenge_method.as_ref() {
                    if !client.code_challenge_methods_supported.contains(code_challenge_method) {
                        return Err(InteractiveAuthorizationError::InvalidCodeChallengeMethodError);
                    }
                } else {
                    return Err(InteractiveAuthorizationError::MissingCodeChallengeError);
                }

                // todo: add code_challenge validation
            }
        }

        let oauth2_authorization_request_id = uuid::Uuid::new_v4().urn().to_string();

        // TODO: Make this configurable?
        let expires_in = 3600; // 1 hour
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let command = OAuth2AuthorizationRequestCommand::CreateOAuth2AuthorizationRequest {
            oauth2_authorization_request_id: oauth2_authorization_request_id.clone(),
            pushed_authorization_request: authorization_request.clone(),
            expires_at,
            interaction_type: Some(InteractionType::OpenId4VpPresentation),
        };

        command_handler(
            &oauth2_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?;

        let oauth2_authorization_request_view = query_handler(
            &oauth2_authorization_request_id,
            &state.query.oauth2_authorization_request,
        )
        .await
        .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?
        .ok_or(InteractiveAuthorizationError::Internal(
            "Failed to retrieve created OAuth2 authorization request".to_string(),
        ))?;

        let openid4vp_request =
            oauth2_authorization_request_view
                .openid4vp_request
                .ok_or(InteractiveAuthorizationError::Internal(
                    "Failed to retrieve OpenID4VP request from OAuth2 authorization request".to_string(),
                ))?;

        Ok(InteractiveAuthorizationResponse {
            status: InteractiveAuthorizationStatus::RequireInteraction,
            code: None,
            interaction_type: Some(InteractionType::OpenId4VpPresentation),
            auth_session: Some(oauth2_authorization_request_id),
            openid4vp_request: Some(openid4vp_request),
            request_uri: None,
            expires_in: None,
        })
    }

    pub async fn handle_interactive_authorization_request_follow_up(
        state: &AuthorizationState,
        auth_session: String,
        openid4vp_response: Option<serde_json::Value>,
        // TODO: support PKCE?
        _code_verifier: Option<String>,
    ) -> Result<InteractiveAuthorizationResponse, InteractiveAuthorizationError> {
        let oauth2_authorization_request_id = auth_session.clone();

        let command = OAuth2AuthorizationRequestCommand::SubmitOpenId4VpResponse {
            openid4vp_response: openid4vp_response
                .clone()
                .ok_or(InteractiveAuthorizationError::MissingOpenId4VPResponseError)?,
        };

        command_handler(
            &oauth2_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?;

        // Get the OAuth2 authorization request that has been pushed via the `/auth/par` endpoint.
        let oauth2_authorization_request = query_handler(
            &oauth2_authorization_request_id,
            &state.query.oauth2_authorization_request,
        )
        .await
        .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?
        .ok_or(InteractiveAuthorizationError::RequestNotFound)?;

        if chrono::Utc::now().timestamp() > oauth2_authorization_request.expires_at {
            return Err(InteractiveAuthorizationError::ExpiredAuthorizationRequestError);
        }

        // TODO: make this configurable?
        let expires_in = 600; // 10 minutes

        let authorization_code_id = uuid::Uuid::new_v4().to_string();
        let command = AuthorizationCodeCommand::CreateAuthorizationCode {
            authorization_code_id: authorization_code_id.clone(),
            client_id: oauth2_authorization_request.client_id.clone(),
            redirect_uri: oauth2_authorization_request.redirect_uri.clone(),
            code_challenge: oauth2_authorization_request.code_challenge,
            code_challenge_method: oauth2_authorization_request.code_challenge_method,
            issuer_state: oauth2_authorization_request.issuer_state,
            expires_in,
        };

        command_handler(&authorization_code_id, &state.command.authorization_code, command)
            .await
            .map_err(|err| InteractiveAuthorizationError::Internal(err.to_string()))?;

        Ok(InteractiveAuthorizationResponse {
            status: InteractiveAuthorizationStatus::Ok,
            code: Some(authorization_code_id),
            interaction_type: None,
            auth_session: None,
            openid4vp_request: None,
            request_uri: None,
            expires_in: Some(expires_in),
        })
    }
}
