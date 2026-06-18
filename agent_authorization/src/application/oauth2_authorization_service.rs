use crate::{
    domain::{
        authorization_code::command::AuthorizationCodeCommand, oauth2_authorization_request::aggregate::ConsentStatus,
    },
    state::AuthorizationState,
};
use agent_shared::handlers::{command_handler, query_handler};
use oid4vc_core::utils::form_urlencoded::to_form_urlencoded_string;
use oid4vci::wallet::AuthorizationRequestByReference;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Deserialize, Serialize)]
pub struct GetConsentQuery {
    pub request_uri: String,
}

pub enum OAuth2AuthorizationServiceResponse {
    RedirectToConsent(String),
    RedirectToClient(Url),
}

// TODO: improve error handling
#[derive(Debug, Error)]
pub enum OAuth2AuthorizationError {
    #[error("Request not found or expired")]
    RequestNotFound,
    #[error("Expired authorization request")]
    ExpiredAuthorizationRequestError,
    #[error("Invalid client ID")]
    InvalidClientIdError,
    #[error("Missing redirect URI")]
    MissingRedirectUriError,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct OAuth2AuthorizationService {}

impl OAuth2AuthorizationService {
    pub async fn handle_authorization_request(
        state: &AuthorizationState,
        AuthorizationRequestByReference { client_id, request_uri }: AuthorizationRequestByReference,
    ) -> Result<OAuth2AuthorizationServiceResponse, OAuth2AuthorizationError> {
        // Get the OAuth2 authorization request that has been pushed via the `/auth/par` endpoint.
        let oauth2_authorization_request = query_handler(
            request_uri.to_string().as_ref(),
            &state.query.oauth2_authorization_request,
        )
        .await
        .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?
        .ok_or(OAuth2AuthorizationError::RequestNotFound)?;

        if chrono::Utc::now().timestamp() > oauth2_authorization_request.expires_at {
            return Err(OAuth2AuthorizationError::ExpiredAuthorizationRequestError);
        }

        if client_id != oauth2_authorization_request.client_id {
            return Err(OAuth2AuthorizationError::InvalidClientIdError);
        }

        match oauth2_authorization_request.consent_status {
            ConsentStatus::Pending => {
                let get_login_query = GetConsentQuery { request_uri };

                let encoded = to_form_urlencoded_string(&get_login_query)
                    .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?;

                Ok(OAuth2AuthorizationServiceResponse::RedirectToConsent(format!(
                    "/auth/consent?{encoded}"
                )))
            }
            ConsentStatus::Expired => Err(OAuth2AuthorizationError::ExpiredAuthorizationRequestError),
            ConsentStatus::Granted => {
                let redirect_uri = oauth2_authorization_request
                    .redirect_uri
                    .ok_or(OAuth2AuthorizationError::MissingRedirectUriError)?;

                let authorization_code_id = uuid::Uuid::new_v4().to_string();
                let command = AuthorizationCodeCommand::CreateAuthorizationCode {
                    authorization_code_id: authorization_code_id.clone(),
                    client_id,
                    redirect_uri: Some(redirect_uri.clone()),
                    code_challenge: oauth2_authorization_request.code_challenge,
                    code_challenge_method: oauth2_authorization_request.code_challenge_method,
                    issuer_state: oauth2_authorization_request.issuer_state,
                    // TODO: make this configurable?
                    expires_in: 600, // 10 minutes
                };

                command_handler(&authorization_code_id, &state.command.authorization_code, command)
                    .await
                    .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?;

                let state = oauth2_authorization_request.state;

                let redirect_uri_with_code = redirect_uri
                    .join(&format!("?code={authorization_code_id}&state={state}"))
                    .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?;

                Ok(OAuth2AuthorizationServiceResponse::RedirectToClient(
                    redirect_uri_with_code,
                ))
            }
            ConsentStatus::Rejected => {
                // TODO: where to redirect the user?
                Err(OAuth2AuthorizationError::Internal(
                    "Consent was rejected, but no redirect URI is specified".to_string(),
                ))
            }
        }
    }
}
