use crate::{
    error::{type_url, IntoApiErrorExt},
    DOCUMENTATION_URL,
};
use agent_issuance::{
    application::access_token_validation_service::AccessTokenValidationError, credential::error::CredentialError,
    offer::error::OfferError, public_offer::error::PublicOfferError, server_config::error::ServerConfigError,
    status_list::error::StatusListError,
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

// TODO: Clearly indicate which errors can occur in the API endpoints (ApiError) and which in the Public endpoints (PublicError).
// Add problem details in the docs for the ApiErrors and ref them via type_url.
impl IntoApiErrorExt for StatusListError {
    fn into_api_error(self) -> ApiError {
        use StatusListError::*;
        match self {
            AggregateNotFound => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            FailedToSetIndex(_, _) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            GzipCompressionError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            JwtEncodeError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            StatusListEncodingError(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            StatusListNotFound(_) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            StatusListQueryError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
            StatusListUrlParsingError => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl IntoApiErrorExt for PublicOfferError {
    fn into_api_error(self) -> ApiError {
        use PublicOfferError::*;

        match self {
            AlreadyExists => ApiError::new(StatusCode::CONFLICT),
            NotFound => ApiError::new(StatusCode::NOT_FOUND),
            TemplateNotFound => ApiError::new(StatusCode::NOT_FOUND),
            TemplateNotEligible => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Template Not Eligible for Public Offer")
                .type_url(type_url("issuance#template-not-eligible-for-public-offer"))
                .message("Public offers require templates that only contain constant values.")
                .finish(),
        }
    }
}

pub enum PublicError {
    AuthorizationError(OID4VCError<AuthorizationErrorResponse>),
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
            // Returned instead of redirecting, because the redirect URI cannot be trusted when the authorization
            // request itself is invalid (RFC 6749, section 4.1.2.1).
            PublicError::AuthorizationError(oid4vc_error) => {
                (StatusCode::BAD_REQUEST, axum::Json(oid4vc_error)).into_response()
            }
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
            PublicError::AccessTokenError(_) => (
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Bearer error=\"invalid_token\"")],
                Json(serde_json::json!({"error": "invalid_token"})),
            )
                .into_response(),
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
            // `/auth/token` endpoint
            MissingTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidRequest)),
            InvalidTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidGrant)),
            InvalidPreAuthorizedCodeError => {
                PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidGrant))
            }
            UnrequestedTxCodeError => PublicError::TokenError(OID4VCError::new(TokenErrorResponse::InvalidRequest)),

            // `/openid4vci/credential` endpoint
            MissingCredentialOfferError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidCredentialRequest))
            }
            MissingCredentialError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidCredentialRequest))
            }
            MissingProofError => PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidProof)),
            InvalidProofError(_) => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidProof))
            }
            MissingProofIssuerError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidProof))
            }
            MissingCredentialConfigurationIdsError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::InvalidCredentialRequest))
            }
            UnknownCredentialConfiguration(_) => PublicError::CredentialError(OID4VCError::new(
                CredentialErrorResponse::UnknownCredentialConfiguration,
            )),
            UnsupportedCredentialIdentifierError => {
                PublicError::CredentialError(OID4VCError::new(CredentialErrorResponse::UnknownCredentialIdentifier))
            }
            // Internal errors that shouldn't reach the public API
            SendCredentialOfferError(_) => PublicError::InternalServerError,
            UnsupportedTokenRequestGrantTypeError => PublicError::InternalServerError,
            InvalidCredentialOfferUriError(_) => PublicError::InternalServerError,
        }
    }
}

impl IntoPublicError for PublicOfferError {
    fn into_public_error(self) -> PublicError {
        match self {
            PublicOfferError::NotFound => PublicError::NotFoundError,
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
            AggregateNotFound => PublicError::InternalServerError,
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

impl From<AuthorizationErrorResponse> for PublicError {
    fn from(err: AuthorizationErrorResponse) -> Self {
        PublicError::AuthorizationError(OID4VCError::new(err))
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
    use crate::error::tests::{assert_problems, into_json_value};
    use crate::DOCUMENTATION_URL;
    use agent_library::json_schema_validation::JsonSchemaError;
    use oauth_tsl::error::OAuthTSLError;
    use serde_json::{json, Value};

    const UNEXPECTED: Option<&str> = Some("unexpected#unexpected-error");

    async fn public_response(error: PublicError) -> (StatusCode, Value) {
        let response = error.into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// Asserts the status code and OpenID4VCI `error` code of each public error; `None` means an empty body.
    pub async fn assert_public_errors<E: IntoPublicError>(cases: Vec<(E, StatusCode, Option<&str>)>) {
        for (error, status, error_code) in cases {
            let description = error.to_string();
            let (actual_status, body) = public_response(error.into_public_error()).await;

            assert_eq!(actual_status, status, "{description}");
            assert_eq!(body.get("error").and_then(Value::as_str), error_code, "{description}");
        }
    }

    #[test]
    fn credential_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                CredentialError::UnsupportedCredentialFormat(json!("ldp_vc")),
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("issuance#unsupported-credential-format"),
            ),
            (
                CredentialError::InvalidCredentialPayloadError(JsonSchemaError::InvalidJsonData("{".to_string())),
                StatusCode::BAD_REQUEST,
                Some("issuance#invalid-credential-payload"),
            ),
            (
                CredentialError::InvalidIdentifierError,
                StatusCode::BAD_REQUEST,
                Some("issuance#invalid-identifier"),
            ),
            (
                CredentialError::InvalidExpirationDateError,
                StatusCode::BAD_REQUEST,
                Some("issuance#invalid-expiration-date"),
            ),
            (
                CredentialError::InvalidCredentialStatus,
                StatusCode::INTERNAL_SERVER_ERROR,
                UNEXPECTED,
            ),
            (
                CredentialError::BuildCredentialError("error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                UNEXPECTED,
            ),
            (
                CredentialError::InvalidCredentialDataError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                CredentialError::InvalidIssuerDidError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (CredentialError::KeyIdError, StatusCode::INTERNAL_SERVER_ERROR, None),
        ]);
    }

    #[test]
    fn offer_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                OfferError::MissingCredentialOfferError,
                StatusCode::BAD_REQUEST,
                Some("issuance#missing-credential-offer"),
            ),
            (
                OfferError::InvalidCredentialOfferUriError(url::ParseError::EmptyHost),
                StatusCode::INTERNAL_SERVER_ERROR,
                UNEXPECTED,
            ),
        ]);

        // These only occur on the OpenID4VCI endpoints, which map them to `PublicError`s instead.
        assert_problems(
            [
                OfferError::UnsupportedTokenRequestGrantTypeError,
                OfferError::MissingTxCodeError,
                OfferError::InvalidTxCodeError,
                OfferError::InvalidPreAuthorizedCodeError,
                OfferError::UnrequestedTxCodeError,
                OfferError::MissingCredentialError,
                OfferError::MissingProofError,
                OfferError::InvalidProofError("error".to_string()),
                OfferError::MissingProofIssuerError,
                OfferError::MissingCredentialConfigurationIdsError,
                OfferError::UnknownCredentialConfiguration("id".to_string()),
                OfferError::UnsupportedCredentialIdentifierError,
            ]
            .map(|error| (error, StatusCode::INTERNAL_SERVER_ERROR, None)),
        );
    }

    #[test]
    fn server_config_status_list_and_public_offer_errors_successfully_convert_to_problem_details() {
        assert_problems([(
            ServerConfigError::UnsupportedCredentialFormatIdentifierError("ldp_vc".to_string()),
            StatusCode::BAD_REQUEST,
            Some("issuance#unsupported-credential-format-identifier-error"),
        )]);

        assert_problems(
            [
                StatusListError::AggregateNotFound,
                StatusListError::FailedToSetIndex(0, "error".to_string()),
                StatusListError::GzipCompressionError,
                StatusListError::JwtEncodeError,
                StatusListError::StatusListEncodingError(OAuthTSLError::InvalidContentType),
                StatusListError::StatusListNotFound("0".to_string()),
                StatusListError::StatusListQueryError,
                StatusListError::StatusListUrlParsingError,
            ]
            .map(|error| (error, StatusCode::INTERNAL_SERVER_ERROR, None)),
        );

        assert_problems([
            (PublicOfferError::AlreadyExists, StatusCode::CONFLICT, None),
            (PublicOfferError::NotFound, StatusCode::NOT_FOUND, None),
            (PublicOfferError::TemplateNotFound, StatusCode::NOT_FOUND, None),
            (
                PublicOfferError::TemplateNotEligible,
                StatusCode::BAD_REQUEST,
                Some("issuance#template-not-eligible-for-public-offer"),
            ),
        ]);
    }

    #[tokio::test]
    async fn offer_errors_successfully_convert_to_public_errors() {
        assert_public_errors(vec![
            (
                OfferError::MissingTxCodeError,
                StatusCode::BAD_REQUEST,
                Some("invalid_request"),
            ),
            (
                OfferError::InvalidTxCodeError,
                StatusCode::BAD_REQUEST,
                Some("invalid_grant"),
            ),
            (
                OfferError::InvalidPreAuthorizedCodeError,
                StatusCode::BAD_REQUEST,
                Some("invalid_grant"),
            ),
            (
                OfferError::UnrequestedTxCodeError,
                StatusCode::BAD_REQUEST,
                Some("invalid_request"),
            ),
            (
                OfferError::MissingCredentialOfferError,
                StatusCode::BAD_REQUEST,
                Some("invalid_credential_request"),
            ),
            (
                OfferError::MissingCredentialError,
                StatusCode::BAD_REQUEST,
                Some("invalid_credential_request"),
            ),
            (
                OfferError::MissingProofError,
                StatusCode::BAD_REQUEST,
                Some("invalid_proof"),
            ),
            (
                OfferError::InvalidProofError("error".to_string()),
                StatusCode::BAD_REQUEST,
                Some("invalid_proof"),
            ),
            (
                OfferError::MissingProofIssuerError,
                StatusCode::BAD_REQUEST,
                Some("invalid_proof"),
            ),
            (
                OfferError::MissingCredentialConfigurationIdsError,
                StatusCode::BAD_REQUEST,
                Some("invalid_credential_request"),
            ),
            (
                OfferError::UnknownCredentialConfiguration("id".to_string()),
                StatusCode::BAD_REQUEST,
                Some("unknown_credential_configuration"),
            ),
            (
                OfferError::UnsupportedCredentialIdentifierError,
                StatusCode::BAD_REQUEST,
                Some("unknown_credential_identifier"),
            ),
            (
                OfferError::UnsupportedTokenRequestGrantTypeError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                OfferError::InvalidCredentialOfferUriError(url::ParseError::EmptyHost),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ])
        .await;
    }

    #[tokio::test]
    async fn internal_issuance_errors_are_not_exposed_through_public_errors() {
        assert_public_errors(
            [
                CredentialError::UnsupportedCredentialFormat(json!("ldp_vc")),
                CredentialError::InvalidCredentialPayloadError(JsonSchemaError::InvalidJsonData("{".to_string())),
                CredentialError::InvalidIdentifierError,
                CredentialError::InvalidCredentialDataError,
                CredentialError::InvalidExpirationDateError,
                CredentialError::InvalidCredentialStatus,
                CredentialError::BuildCredentialError("error".to_string()),
                CredentialError::InvalidIssuerDidError,
                CredentialError::KeyIdError,
            ]
            .map(|error| (error, StatusCode::INTERNAL_SERVER_ERROR, None))
            .into(),
        )
        .await;

        assert_public_errors(
            [
                ServerConfigError::UpdateProvisionedCredentialConfigurationError,
                ServerConfigError::RemoveProvisionedCredentialConfigurationError,
                ServerConfigError::UnsupportedCredentialFormatIdentifierError("ldp_vc".to_string()),
            ]
            .map(|error| (error, StatusCode::INTERNAL_SERVER_ERROR, None))
            .into(),
        )
        .await;

        assert_public_errors(vec![
            (PublicOfferError::NotFound, StatusCode::NOT_FOUND, None),
            (PublicOfferError::AlreadyExists, StatusCode::INTERNAL_SERVER_ERROR, None),
        ])
        .await;

        assert_public_errors(vec![
            (
                StatusListError::StatusListNotFound("0".to_string()),
                StatusCode::NOT_FOUND,
                None,
            ),
            (
                StatusListError::AggregateNotFound,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                StatusListError::FailedToSetIndex(0, "error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                StatusListError::GzipCompressionError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (StatusListError::JwtEncodeError, StatusCode::INTERNAL_SERVER_ERROR, None),
            (
                StatusListError::StatusListEncodingError(OAuthTSLError::InvalidContentType),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                StatusListError::StatusListQueryError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                StatusListError::StatusListUrlParsingError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ])
        .await;
    }

    #[tokio::test]
    async fn public_errors_successfully_convert_to_responses() {
        let (status, body) = public_response(AuthorizationErrorResponse::InvalidRequest.into()).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_request"))
        );

        let (status, body) = public_response(TokenErrorResponse::InvalidClient.into()).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("invalid_client"))
        );

        let (status, body) = public_response(NotificationErrorResponse::InvalidNotificationId.into()).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_notification_id"))
        );

        let response = PublicError::from(AccessTokenValidationError::InvalidToken).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["WWW-Authenticate"], "Bearer error=\"invalid_token\"");
        assert_eq!(into_json_value(response).await, json!({ "error": "invalid_token" }));

        let (status, _) = public_response(internal_server_error()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

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
