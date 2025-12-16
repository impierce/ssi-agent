use crate::issuance::error::{IntoPublicError, PublicError};
use agent_authorization::application::token_issuance_service::TokenIssuanceError;
use oid4vci::errors::TokenErrorResponse;

impl IntoPublicError for TokenIssuanceError {
    fn into_public_error(self) -> PublicError {
        use TokenIssuanceError::*;
        match self {
            InvalidClientIdError => PublicError::from(TokenErrorResponse::InvalidClient),
            InvalidAuthorizationCodeError(_) => PublicError::from(TokenErrorResponse::InvalidGrant),
            MissingAuthorizationCodeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingTxCodeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidTxCodeError => PublicError::from(TokenErrorResponse::InvalidGrant),
            InvalidPreAuthorizedCodeError => PublicError::from(TokenErrorResponse::InvalidGrant),
            UnrequestedTxCodeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingAccessTokenError => PublicError::from(TokenErrorResponse::InvalidRequest),
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<TokenIssuanceError> for PublicError {
    fn from(err: TokenIssuanceError) -> Self {
        err.into_public_error()
    }
}
