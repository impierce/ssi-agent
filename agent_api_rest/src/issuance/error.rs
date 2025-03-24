use crate::error::IntoApiErrorExt;
use crate::DOCUMENTATION_URL;
use agent_issuance::{
    credential::error::CredentialError, offer::error::OfferError, server_config::error::ServerConfigError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

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
                .title("Invalid Credential Offer URI")
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-credential-offer-uri"
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
                "detail": "Credential Offer is does not exist"
            }),
        );
    }
}
