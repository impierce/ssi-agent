use crate::{
    domain::oauth2_authorization_request::command::OAuth2AuthorizationRequestCommand, state::AuthorizationState,
};
use agent_shared::handlers::{command_handler, public_query_handler};
use thiserror::Error;

pub enum ConsentServiceResponse {
    Found(String),
    // TODO: where to redirect to if consent is not given?
}

// TODO: improve error handling
#[derive(Debug, Error)]
pub enum ConsentError {
    #[error("Request not found or expired")]
    RequestNotFound,
    #[error("Client not found")]
    ClientNotFound,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct ConsentService {}

impl ConsentService {
    pub async fn handle_consent(
        state: &AuthorizationState,
        client_id: String,
        request_uri: String,
        consent_given: bool,
    ) -> Result<ConsentServiceResponse, ConsentError> {
        let oauth_authorization_request_id = request_uri.clone();

        let _oauth_authorization_request = public_query_handler(
            &oauth_authorization_request_id,
            &state.query.oauth2_authorization_request,
        )
        .await
        .map_err(|err| ConsentError::Internal(err.to_string()))?
        .ok_or(ConsentError::RequestNotFound)?;

        let command = if consent_given {
            OAuth2AuthorizationRequestCommand::GrantConsent
        } else {
            OAuth2AuthorizationRequestCommand::RejectConsent
        };

        command_handler(
            state.authorization_checker.clone(),
            None,
            &oauth_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .map_err(|err| ConsentError::Internal(err.to_string()))?;

        let encoded_request_uri = urlencoding::encode(&request_uri).clone();

        Ok(ConsentServiceResponse::Found(format!(
            "/auth/authorize?client_id={client_id}&request_uri={encoded_request_uri}",
        )))
    }
}
