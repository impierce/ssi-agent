use crate::{
    domain::oauth2_authorization_request::{
        aggregate::ConsentStatus, command::OAuth2AuthorizationRequestCommand, error::OAuth2AuthorizationRequestError,
    },
    state::AuthorizationState,
};
use agent_shared::handlers::{public_command_handler, public_query_handler, CommandHandlerError};
use cqrs_es::AggregateError;
use thiserror::Error;

pub enum ConsentServiceResponse {
    Found(String),
}

// TODO: improve error handling
#[derive(Debug, Error)]
pub enum ConsentError {
    #[error("Request not found or expired")]
    RequestNotFound,
    #[error("Client not found")]
    ClientNotFound,
    #[error("The authorization request belongs to another client")]
    ClientMismatch,
    #[error("Consent has already been given or denied")]
    ConsentAlreadyDecided,
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

        let oauth_authorization_request = public_query_handler(
            &oauth_authorization_request_id,
            &state.query.oauth2_authorization_request,
        )
        .await
        .map_err(|err| ConsentError::Internal(err.to_string()))?
        .ok_or(ConsentError::RequestNotFound)?;

        if oauth_authorization_request.client_id != client_id {
            return Err(ConsentError::ClientMismatch);
        }

        // Pushed authorization requests of unregistered clients are accepted (see `PushedAuthorizationService`), but
        // only registered clients can be given consent.
        public_query_handler(&client_id, &state.query.client)
            .await
            .map_err(|err| ConsentError::Internal(err.to_string()))?
            .ok_or(ConsentError::ClientNotFound)?;

        if matches!(
            oauth_authorization_request.consent_status,
            ConsentStatus::Granted | ConsentStatus::Rejected
        ) {
            return Err(ConsentError::ConsentAlreadyDecided);
        }

        let command = if consent_given {
            OAuth2AuthorizationRequestCommand::GrantConsent
        } else {
            OAuth2AuthorizationRequestCommand::RejectConsent
        };

        // The aggregate refuses a second decision, so of two concurrent decisions only the first is recorded.
        public_command_handler(
            &oauth_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .map_err(|err| match err {
            CommandHandlerError::Aggregate(
                AggregateError::UserError(OAuth2AuthorizationRequestError::ConsentAlreadyDecided)
                | AggregateError::AggregateConflict,
            ) => ConsentError::ConsentAlreadyDecided,
            err => ConsentError::Internal(err.to_string()),
        })?;

        Ok(ConsentServiceResponse::Found(format!(
            "/auth/authorize?client_id={}&request_uri={}",
            urlencoding::encode(&client_id),
            urlencoding::encode(&request_uri),
        )))
    }
}
