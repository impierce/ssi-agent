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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v0::issuance::error::tests::assert_public_errors;
    use http::StatusCode;

    const INVALID_REQUEST: Option<&str> = Some("invalid_request");
    const INVALID_GRANT: Option<&str> = Some("invalid_grant");
    const INVALID_CLIENT: Option<&str> = Some("invalid_client");

    #[tokio::test]
    async fn token_issuance_errors_successfully_convert_to_public_errors() {
        use TokenIssuanceError::*;

        assert_public_errors(vec![
            (InvalidClientIdError, StatusCode::UNAUTHORIZED, INVALID_CLIENT),
            (
                InvalidAuthorizationCodeError("error".to_string()),
                StatusCode::BAD_REQUEST,
                INVALID_GRANT,
            ),
            (MissingAuthorizationCodeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (MissingTxCodeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (InvalidTxCodeError, StatusCode::BAD_REQUEST, INVALID_GRANT),
            (InvalidPreAuthorizedCodeError, StatusCode::BAD_REQUEST, INVALID_GRANT),
            (InactivePublicOfferError, StatusCode::BAD_REQUEST, INVALID_GRANT),
            (UnrequestedTxCodeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (MissingAccessTokenError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (Internal("error".to_string()), StatusCode::INTERNAL_SERVER_ERROR, None),
        ])
        .await;
    }

    #[tokio::test]
    async fn pushed_authorization_errors_successfully_convert_to_public_errors() {
        use PushedAuthorizationError::*;

        assert_public_errors(vec![
            (InvalidClientIdError, StatusCode::UNAUTHORIZED, INVALID_CLIENT),
            (InvalidRedirectUriError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (InvalidResponseTypeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (MissingCodeChallengeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (
                InvalidCodeChallengeMethodError,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (Internal("error".to_string()), StatusCode::INTERNAL_SERVER_ERROR, None),
        ])
        .await;
    }

    #[tokio::test]
    async fn interactive_authorization_errors_successfully_convert_to_public_errors() {
        use InteractiveAuthorizationError::*;

        assert_public_errors(vec![
            (InvalidClientIdError, StatusCode::UNAUTHORIZED, INVALID_CLIENT),
            (InvalidRedirectUriError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (InvalidResponseTypeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (MissingCodeChallengeError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (
                InvalidCodeChallengeMethodError,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (RequestNotFound, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (
                ExpiredAuthorizationRequestError,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (MissingOpenId4VPResponseError, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (
                InvalidOpenId4VPResponseError("error".to_string()),
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (
                UnsupportedInteractionTypesError("error".to_string()),
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (MissingRedirectUriError, StatusCode::INTERNAL_SERVER_ERROR, None),
            (Internal("error".to_string()), StatusCode::INTERNAL_SERVER_ERROR, None),
        ])
        .await;
    }

    /// Authorization endpoint errors are returned as `400` instead of redirecting, since the redirect URI of an
    /// invalid authorization request cannot be trusted.
    #[tokio::test]
    async fn authorization_endpoint_errors_successfully_convert_to_public_errors() {
        assert_public_errors(vec![
            (
                OAuth2AuthorizationError::RequestNotFound,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (
                OAuth2AuthorizationError::ExpiredAuthorizationRequestError,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (
                OAuth2AuthorizationError::InvalidClientIdError,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (
                OAuth2AuthorizationError::MissingRedirectUriError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                OAuth2AuthorizationError::Internal("error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ])
        .await;

        assert_public_errors(vec![
            (
                ConsentQueryError::RequestNotFound,
                StatusCode::BAD_REQUEST,
                INVALID_REQUEST,
            ),
            (
                ConsentQueryError::ClientNotFound,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                ConsentQueryError::Internal("error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ])
        .await;

        assert_public_errors(vec![
            (ConsentError::RequestNotFound, StatusCode::BAD_REQUEST, INVALID_REQUEST),
            (ConsentError::ClientNotFound, StatusCode::INTERNAL_SERVER_ERROR, None),
            (
                ConsentError::Internal("error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ])
        .await;
    }
}
