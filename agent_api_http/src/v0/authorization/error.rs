use crate::v0::issuance::error::{IntoPublicError, PublicError};
use agent_authorization::application::{
    consent_query_service::ConsentQueryError, consent_service::ConsentError,
    interactive_authorization_service::InteractiveAuthorizationError,
    oauth2_authorization_service::OAuth2AuthorizationError, pushed_authorization_service::PushedAuthorizationError,
    token_issuance_service::TokenIssuanceError,
};
use oid4vci::errors::{AuthorizationErrorResponse, TokenErrorResponse};

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
            InactivePublicOfferError => PublicError::from(TokenErrorResponse::InvalidGrant),
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

impl IntoPublicError for PushedAuthorizationError {
    fn into_public_error(self) -> PublicError {
        use PushedAuthorizationError::*;
        match self {
            InvalidClientIdError => PublicError::from(TokenErrorResponse::InvalidClient),
            InvalidRedirectUriError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidResponseTypeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingCodeChallengeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidCodeChallengeMethodError => PublicError::from(TokenErrorResponse::InvalidRequest),
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<PushedAuthorizationError> for PublicError {
    fn from(err: PushedAuthorizationError) -> Self {
        err.into_public_error()
    }
}

impl IntoPublicError for InteractiveAuthorizationError {
    fn into_public_error(self) -> PublicError {
        use InteractiveAuthorizationError::*;
        match self {
            InvalidClientIdError => PublicError::from(TokenErrorResponse::InvalidClient),
            InvalidRedirectUriError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidResponseTypeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingCodeChallengeError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidCodeChallengeMethodError => PublicError::from(TokenErrorResponse::InvalidRequest),
            RequestNotFound => PublicError::from(TokenErrorResponse::InvalidRequest),
            ExpiredAuthorizationRequestError => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingOpenId4VPResponseError => PublicError::from(TokenErrorResponse::InvalidRequest),
            InvalidOpenId4VPResponseError(_) => PublicError::from(TokenErrorResponse::InvalidRequest),
            UnsupportedInteractionTypesError(_) => PublicError::from(TokenErrorResponse::InvalidRequest),
            MissingRedirectUriError => PublicError::InternalServerError,
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<InteractiveAuthorizationError> for PublicError {
    fn from(err: InteractiveAuthorizationError) -> Self {
        err.into_public_error()
    }
}

impl IntoPublicError for OAuth2AuthorizationError {
    fn into_public_error(self) -> PublicError {
        use OAuth2AuthorizationError::*;
        match self {
            RequestNotFound => PublicError::from(AuthorizationErrorResponse::InvalidRequest),
            ExpiredAuthorizationRequestError => PublicError::from(AuthorizationErrorResponse::InvalidRequest),
            InvalidClientIdError => PublicError::from(AuthorizationErrorResponse::InvalidRequest),
            MissingRedirectUriError => PublicError::InternalServerError,
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<OAuth2AuthorizationError> for PublicError {
    fn from(err: OAuth2AuthorizationError) -> Self {
        err.into_public_error()
    }
}

impl IntoPublicError for ConsentQueryError {
    fn into_public_error(self) -> PublicError {
        use ConsentQueryError::*;
        match self {
            RequestNotFound => PublicError::from(AuthorizationErrorResponse::InvalidRequest),
            ClientNotFound => PublicError::InternalServerError,
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<ConsentQueryError> for PublicError {
    fn from(err: ConsentQueryError) -> Self {
        err.into_public_error()
    }
}

impl IntoPublicError for ConsentError {
    fn into_public_error(self) -> PublicError {
        use ConsentError::*;
        match self {
            RequestNotFound => PublicError::from(AuthorizationErrorResponse::InvalidRequest),
            ClientNotFound => PublicError::InternalServerError,
            Internal(_) => PublicError::InternalServerError,
        }
    }
}

impl From<ConsentError> for PublicError {
    fn from(err: ConsentError) -> Self {
        err.into_public_error()
    }
}
