use crate::{
    domain::{
        authorization_code::command::AuthorizationCodeCommand,
        oauth2_authorization_request::aggregate::{ConsentStatus, OAuth2AuthorizationRequest},
    },
    state::AuthorizationState,
};
use agent_shared::handlers::{public_command_handler, public_query_handler};
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
    #[error("The client is not registered, or the redirect URI is not one of its registered redirect URIs")]
    UnauthorizedClient,
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
        let oauth2_authorization_request = public_query_handler(
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
                let redirect_uri = registered_redirect_uri(state, &oauth2_authorization_request).await?;

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

                public_command_handler(&authorization_code_id, &state.command.authorization_code, command)
                    .await
                    .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?;

                Ok(OAuth2AuthorizationServiceResponse::RedirectToClient(
                    redirect_to_client(
                        redirect_uri,
                        ("code", &authorization_code_id),
                        &oauth2_authorization_request.state,
                    ),
                ))
            }
            ConsentStatus::Rejected => {
                // TODO: where to redirect the user?
                let redirect_uri = registered_redirect_uri(state, &oauth2_authorization_request).await?;

                Ok(OAuth2AuthorizationServiceResponse::RedirectToClient(
                    redirect_to_client(
                        redirect_uri,
                        ("error", "access_denied"),
                        &oauth2_authorization_request.state,
                    ),
                ))
            }
        }
    }
}

/// The redirect URI of the authorization request, if its client is registered and lists it. Pushed authorization
/// requests of unregistered clients are accepted (see `PushedAuthorizationService`), so the redirect URI is checked
/// again before anything is sent to it.
async fn registered_redirect_uri(
    state: &AuthorizationState,
    oauth2_authorization_request: &OAuth2AuthorizationRequest,
) -> Result<Url, OAuth2AuthorizationError> {
    let redirect_uri = oauth2_authorization_request
        .redirect_uri
        .clone()
        .ok_or(OAuth2AuthorizationError::MissingRedirectUriError)?;

    let client = public_query_handler(&oauth2_authorization_request.client_id, &state.query.client)
        .await
        .map_err(|err| OAuth2AuthorizationError::Internal(err.to_string()))?
        .ok_or(OAuth2AuthorizationError::UnauthorizedClient)?;

    if !client.redirect_uris.contains(&redirect_uri) {
        return Err(OAuth2AuthorizationError::UnauthorizedClient);
    }

    Ok(redirect_uri)
}

/// Appends the authorization response parameter and, if the request carried one, `state` to the redirect URI, keeping
/// its query (RFC 6749, sections 3.1.2 and 4.1.2).
fn redirect_to_client(mut redirect_uri: Url, (key, value): (&str, &str), state: &str) -> Url {
    {
        let mut query = redirect_uri.query_pairs_mut();
        query.append_pair(key, value);
        if !state.is_empty() {
            query.append_pair("state", state);
        }
    }
    redirect_uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_parameters_are_encoded_and_keep_the_query() {
        let redirect_uri: Url = "https://client.example/cb?tenant=a".parse().unwrap();

        assert_eq!(
            redirect_to_client(redirect_uri.clone(), ("code", "c"), "s&code=evil").as_str(),
            "https://client.example/cb?tenant=a&code=c&state=s%26code%3Devil"
        );
        assert_eq!(
            redirect_to_client(redirect_uri, ("error", "access_denied"), "").as_str(),
            "https://client.example/cb?tenant=a&error=access_denied"
        );
    }
}
