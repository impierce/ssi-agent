use crate::{
    error::{type_url, IntoApiErrorExt},
    DOCUMENTATION_URL,
};
use agent_issuance::{
    application::access_token_validation_service::AccessTokenValidationError, credential::error::CredentialError,
    offer::error::OfferError, server_config::error::ServerConfigError, status_list::error::StatusListError,
};
use axum::{response::IntoResponse, response::Response, Json};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::errors::{
    AuthorizationErrorResponse, CredentialErrorResponse, DeferredCredentialErrorResponse, ErrorStatusCode,
    NotificationErrorResponse, OID4VCError, TokenErrorResponse,
};

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        use CredentialError::*;

        match self {
            // UniCore API Problem Details
            UnsupportedCredentialFormat(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unsupported Credential Format")
                .type_url(type_url("issuance#unsupported-credential-format"))
                .source(self)
                .finish(),
            UnsupportedCredentialType => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unsupported Credential Type")
                .type_url(type_url("issuance#unsupported-credential-type"))
                .source(self)
                .finish(),
            InvalidCredentialPayloadError(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Payload")
                .type_url(type_url("issuance#invalid-credential-payload"))
                .source(self)
                .finish(),
            InvalidIdentifierError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Identifier")
                .type_url(type_url("issuance#invalid-identifier"))
                .source(self)
                .finish(),
            InvalidExpirationDateError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Expiration Date")
                .type_url(type_url("issuance#invalid-expiration-date"))
                .source(self)
                .finish(),
            InvalidCredentialStatus => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),
            BuildCredentialError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),

            // Public API Errors

            // `/openid4vci/credential` endpoint
            InvalidCredentialDataError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidIssuerDidError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            KeyIdError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        use OfferError::*;

        match self {
            // UniCore API Problem Details
            MissingCredentialOfferError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Credential Offer")
                .type_url(type_url("issuance#missing-credential-offer"))
                .source(self)
                .finish(),
            SendCredentialOfferError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Send Credential Offer Error")
                .type_url(type_url("issuance#send-credential-offer-error"))
                .source(self)
                .finish(),
            InvalidCredentialOfferUriError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(type_url("unexpected#unexpected-error"))
                .source(self)
                .finish(),

            // Public API Errors

            // `/auth/token` endpoint
            UnsupportedTokenRequestGrantTypeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingTxCodeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidTxCodeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidPreAuthorizedCodeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            UnrequestedTxCodeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),

            // `/openid4vci/credential` endpoint
            MissingCredentialError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingProofError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidProofError(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingProofIssuerError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingCredentialConfigurationIdsError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            UnknownCredentialConfiguration(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            UnsupportedCredentialIdentifierError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl IntoApiErrorExt for ServerConfigError {
    fn into_api_error(self) -> ApiError {
        use ServerConfigError::*;
        match self {
            // UniCore API Problem Details
            UpdateProvisionedCredentialConfigurationError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Update Provisioned Credential Configuration Error")
                .type_url(type_url("issuance#update-provisioned-credential-configuration-error"))
                .source(self)
                .finish(),
            RemoveProvisionedCredentialConfigurationError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Remove Provisioned Credential Configuration Error")
                .type_url(type_url("issuance#remove-provisioned-credential-configuration-error"))
                .source(self)
                .finish(),
            UnsupportedCredentialFormatIdentifierError(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Unsupported Credential Format Identifier Error")
                .type_url(type_url("issuance#unsupported-credential-format-identifier-error"))
                .source(self)
                .finish(),
        }
    }
}

// TODO: Add problem details in the docs for these errors and ref them via type_url
impl IntoApiErrorExt for StatusListError {
    fn into_api_error(self) -> ApiError {
        use StatusListError::*;
        match self {
            FailedToSetIndex(_, _) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Failed to Set Status List Index")
                .source(self)
                .finish(),
            GzipCompressionError => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Gzip Compression Error")
                .source(self)
                .finish(),
            JwtEncodeError => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("JWT Encode Error")
                .source(self)
                .finish(),
            StatusListEncodingError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Status List Encoding Error")
                .source(self)
                .finish(),
            StatusListNotFound(_) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Status List Not Found")
                .source(self)
                .finish(),
            StatusListQueryError => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Error Querying Status List")
                .source(self)
                .finish(),
            StatusListUrlParsingError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Unable to parse ID segment of Status List URL")
                .source(self)
                .finish(),
        }
    }
}

pub enum PublicError {
    TokenError(OID4VCError<TokenErrorResponse>),
    CredentialError(OID4VCError<CredentialErrorResponse>),
    NotificationError(OID4VCError<NotificationErrorResponse>),
    AccessTokenError(AccessTokenValidationError),
    InternalServerError,
    NotFoundError,
}

impl axum::response::IntoResponse for PublicError {
    fn into_response(self) -> axum::response::Response {
        match self {
            PublicError::TokenError(oid4vc_error) => {
                let status = oid4vc_error.error.status_code();
                (status, axum::Json(oid4vc_error)).into_response()
            }
            PublicError::CredentialError(oid4vc_error) => {
                let status = oid4vc_error.error.status_code();
                (status, axum::Json(oid4vc_error)).into_response()
            }
            PublicError::NotificationError(oid4vc_error) => {
                let status = oid4vc_error.error.status_code();
                (status, axum::Json(oid4vc_error)).into_response()
            }
            PublicError::AccessTokenError(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            PublicError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            PublicError::NotFoundError => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

pub trait IntoPublicError: std::error::Error {
    fn into_public_error(self) -> PublicError;
}

impl IntoPublicError for CredentialError {
    fn into_public_error(self) -> PublicError {
        use CredentialError::*;
        match self {
            UnsupportedCredentialFormat(_) => PublicError::InternalServerError,
            UnsupportedCredentialType => PublicError::InternalServerError,
            InvalidCredentialPayloadError(_) => PublicError::InternalServerError,
            InvalidIdentifierError => PublicError::InternalServerError,
            InvalidCredentialDataError => PublicError::InternalServerError,
            InvalidExpirationDateError => PublicError::InternalServerError,
            InvalidCredentialStatus => PublicError::InternalServerError,
            BuildCredentialError(_) => PublicError::InternalServerError,
            InvalidIssuerDidError => PublicError::InternalServerError,
            KeyIdError => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for OfferError {
    fn into_public_error(self) -> PublicError {
        use OfferError::*;
        match self {
            MissingCredentialOfferError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidCredentialRequest))
            }
            MissingTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidRequest)),
            InvalidTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidGrant)),
            InvalidPreAuthorizedCodeError => {
                PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidGrant))
            }
            UnrequestedTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidRequest)),
            // TODO: check for missing error responses
            _ => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for ServerConfigError {
    fn into_public_error(self) -> PublicError {
        use ServerConfigError::*;
        match self {
            UpdateProvisionedCredentialConfigurationError => PublicError::InternalServerError,
            RemoveProvisionedCredentialConfigurationError => PublicError::InternalServerError,
            UnsupportedCredentialFormatIdentifierError(_) => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for StatusListError {
    fn into_public_error(self) -> PublicError {
        use StatusListError::*;
        match self {
            FailedToSetIndex(_, _) => PublicError::InternalServerError,
            GzipCompressionError => PublicError::InternalServerError,
            JwtEncodeError => PublicError::InternalServerError,
            StatusListEncodingError(_) => PublicError::InternalServerError,
            StatusListNotFound(_) => PublicError::NotFoundError,
            StatusListQueryError => PublicError::InternalServerError,
            StatusListUrlParsingError => PublicError::InternalServerError,
        }
    }
}

impl From<StatusListError> for PublicError {
    fn from(err: StatusListError) -> Self {
        err.into_public_error()
    }
}

impl From<CredentialErrorResponse> for PublicError {
    fn from(err: CredentialErrorResponse) -> Self {
        PublicError::CredentialError(OID4VCError::new(err))
    }
}

impl From<TokenErrorResponse> for PublicError {
    fn from(err: TokenErrorResponse) -> Self {
        PublicError::TokenError(OID4VCError::new(err))
    }
}

impl From<NotificationErrorResponse> for PublicError {
    fn from(err: NotificationErrorResponse) -> Self {
        PublicError::NotificationError(OID4VCError::new(err))
    }
}

impl From<AccessTokenValidationError> for PublicError {
    fn from(err: AccessTokenValidationError) -> Self {
        PublicError::AccessTokenError(err)
    }
}

pub fn authorization_error(error: AuthorizationErrorResponse) -> Response {
    let error: OID4VCError<AuthorizationErrorResponse> = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn token_error(error: TokenErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn credential_error(error: CredentialErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn deferred_credential_error(error: DeferredCredentialErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn notification_error(error: NotificationErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn internal_server_error() -> PublicError {
    PublicError::InternalServerError
}

pub fn access_token_error(err: AccessTokenValidationError) -> PublicError {
    PublicError::AccessTokenError(err)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::tests::into_json_value;
    use crate::DOCUMENTATION_URL;
    use serde_json::json;

    #[tokio::test]
    async fn issuance_errors_successfully_convert_to_problem_details() {
        assert_eq!(
            into_json_value(
                CredentialError::InvalidIdentifierError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#invalid-identifier"),
                "title": "Invalid Identifier",
                "status": 400,
                "detail": "The `id` value could not be parsed to a valid URI"
            }),
        );

        assert_eq!(
            into_json_value(
                CredentialError::InvalidExpirationDateError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#invalid-expiration-date"),
                "title": "Invalid Expiration Date",
                "status": 400,
                "detail": "Invalid expiration data: The expiration date must not exceed `9999-12-31T23:59:59Z`. Please provide a valid date within the supported range."
            }),
        );

        assert_eq!(
            into_json_value(
                OfferError::MissingCredentialOfferError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#missing-credential-offer"),
                "title": "Missing Credential Offer",
                "status": 400,
                "detail": "Credential Offer does not exist"
            }),
        );

        assert_eq!(
            into_json_value(
                ServerConfigError::UpdateProvisionedCredentialConfigurationError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#update-provisioned-credential-configuration-error"),
                "title": "Update Provisioned Credential Configuration Error",
                "status": 400,
                "detail": "Cannot update provisioned credential configuration during runtime"
            }),
        );

        assert_eq!(
            into_json_value(
                ServerConfigError::RemoveProvisionedCredentialConfigurationError
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#remove-provisioned-credential-configuration-error"),
                "title": "Remove Provisioned Credential Configuration Error",
                "status": 400,
                "detail": "Cannot remove provisioned credential configuration during runtime"
            }),
        );
    }
}
