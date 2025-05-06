use crate::error::IntoApiErrorExt;
use crate::DOCUMENTATION_URL;
use agent_issuance::{
    credential::error::CredentialError, offer::error::OfferError, server_config::error::ServerConfigError,
};
use axum::{response::IntoResponse, response::Response, Json};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::errors::{
    AuthorizationErrorResponse, BatchCredentialErrorResponse, CredentialErrorResponse, DeferredCredentialErrorResponse,
    ErrorStatusCode, NotificationErrorResponse, OID4VCError, TokenErrorResponse,
};

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        use CredentialError::*;

        match self {
            // UniCore API Problem Details
            UnsupportedCredentialFormat(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unsupported Credential Format")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-format"
                ))
                .source(self)
                .finish(),
            UnsupportedCredentialType => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unsupported Credential Type")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-type"
                ))
                .source(self)
                .finish(),
            InvalidCredentialSubjectError(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential Subject")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-credential-subject"
                ))
                .source(self)
                .finish(),
            InvalidIdentifierError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Identifier")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-identifier"
                ))
                .source(self)
                .finish(),
            InvalidExpirationDateError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Expiration Date")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-expiration-date"
                ))
                .source(self)
                .finish(),

            // Public API Errors

            // `/openid4vci/credential` endpoint
            MissingCredentialDataError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
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
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#missing-credential-offer"
                ))
                .source(self)
                .finish(),
            SendCredentialOfferError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Send Credential Offer Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#send-credential-offer-error"
                ))
                .source(self)
                .finish(),
            InvalidCredentialOfferUriError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/unexpected#unexpected-error"
                ))
                .source(self)
                .finish(),

            // Public API Errors

            // `/auth/token` endpoint
            UnsupportedTokenRequestGrantTypeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),

            // `/openid4vci/credential` endpoint
            MissingCredentialError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingProofError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            InvalidProofError(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            MissingProofIssuerError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl IntoApiErrorExt for ServerConfigError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}

pub enum PublicError {
    // OID4VCError(OID4VCError<T>),
    TokenError(OID4VCError<TokenErrorResponse>),
    CredentialError(OID4VCError<CredentialErrorResponse>),
    NotificationError(OID4VCError<NotificationErrorResponse>),
    InternalServerError,
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
            PublicError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
pub trait IntoPublicError: std::error::Error {
    fn into_public_error(self) -> PublicError;
}

impl IntoPublicError for CredentialError {
    fn into_public_error(self) -> PublicError
// where
    //     T: From<CredentialErrorResponse>,
    {
        use CredentialError::*;
        match self {
            UnsupportedCredentialFormat(_) => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::UnsupportedCredentialFormat))
            }

            UnsupportedCredentialType => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::UnsupportedCredentialType))
            }

            _ => PublicError::InternalServerError,
            // Public API Errors
            // `/openid4vci/credential` endpoint
            //MissingCredentialDataError => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for OfferError {
    fn into_public_error(self) -> PublicError
// where
    //     T: From<CredentialErrorResponse>,
    {
        use OfferError::*;
        match self {
            MissingCredentialOfferError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidCredentialRequest))
            }
            _ => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for ServerConfigError {
    fn into_public_error(self) -> PublicError {
        match self {}
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

pub fn token_error_to_api_error(error: TokenErrorResponse) -> ApiError {
    let status = error.status_code();
    ApiError::builder(status)
        .title(format!("OID4VCI Error: {}", error))
        .source(error)
        .finish()
}

pub fn credential_error(error: CredentialErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn credential_error_to_api_error(error: CredentialErrorResponse) -> ApiError {
    let status = error.status_code();
    //You could add .type_url for documentation URL (if needed)
    ApiError::builder(status)
        .title(format!("OID4VCI Error: {}", error))
        .source(error)
        .finish()
}

pub fn batch_credential_error(error: BatchCredentialErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn batch_credential_error_to_api_error(error: BatchCredentialErrorResponse) -> ApiError {
    let status = error.status_code();
    ApiError::builder(status)
        .title(format!("OID4VCI Error: {}", error))
        .source(error)
        .finish()
}

pub fn deferred_credential_error(error: DeferredCredentialErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}
pub fn deferred_credential_error_to_api_error(error: DeferredCredentialErrorResponse) -> ApiError {
    let status = error.status_code();

    ApiError::builder(status)
        .title(format!("OID4VCI Error: {}", error))
        .source(error)
        .finish()
}

pub fn notification_error(error: NotificationErrorResponse) -> Response {
    let error = OID4VCError::new(error);
    let status = error.error.status_code();
    (status, Json(error)).into_response()
}

pub fn internal_server_error() -> PublicError {
    PublicError::InternalServerError
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::tests::into_json_value;
    use serde_json::json;

    #[tokio::test]
    async fn issuance_errors_successfully_convert_to_problem_details() {
        assert_eq!(
            into_json_value(
                CredentialError::UnsupportedCredentialFormat(serde_json::json!("unsupported_format"))
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-format"),
                "title": "Unsupported Credential Format",
                "status": 500,
                "detail": "Credential format not supported: `\"unsupported_format\"`"
            }),
        );

        assert_eq!(
            into_json_value(
                CredentialError::UnsupportedCredentialType
                    .into_api_error()
                    .into_axum_response()
            )
            .await,
            json!({
                "type": format!("{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-type"),
                "title": "Unsupported Credential Type",
                "status": 500,
                "detail": "This Credential type is not supported"
            }),
        );

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
    }
}
