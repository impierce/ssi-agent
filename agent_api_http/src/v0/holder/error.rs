use crate::error::{type_url, IntoApiErrorExt};
use agent_holder::{
    credential::error::CredentialError, offer::error::OfferError, presentation::error::PresentationError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for CredentialError {
    fn into_api_error(self) -> ApiError {
        match self {
            CredentialError::CredentialDecodingError => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Credential Decoding Failed")
                .type_url(type_url("holder#credential-decoding-failed"))
                .message("The supplied credential could not be decoded as a JWT verifiable credential.")
                .source(self)
                .finish(),
            CredentialError::InvalidCredentialStatus => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Invalid Credential Status")
                .type_url(type_url("holder#invalid-credential-status"))
                .message("The credential's status could not be verified, or the credential is no longer valid.")
                .source(self)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        match self {
            OfferError::MissingCredentialOfferError => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Credential Offer Not Found")
                .type_url(type_url("holder#credential-offer-not-found"))
                .message("No credential offer was received under this ID.")
                .source(self)
                .finish(),
            OfferError::CredentialOfferStatusNotPendingError => ApiError::builder(StatusCode::CONFLICT)
                .title("Credential Offer Not Pending")
                .type_url(type_url("holder#credential-offer-not-pending"))
                .message("The credential offer has already been accepted or rejected.")
                .source(self)
                .finish(),
            OfferError::CredentialOfferStatusNotAcceptedError => ApiError::builder(StatusCode::CONFLICT)
                .title("Credential Offer Not Accepted")
                .type_url(type_url("holder#credential-offer-not-accepted"))
                .message("The credential offer must be accepted before credentials can be requested.")
                .source(self)
                .finish(),
            OfferError::MissingTokenResponseError => ApiError::builder(StatusCode::CONFLICT)
                .title("Missing Token Response")
                .type_url(type_url("holder#missing-token-response"))
                .message("The credential offer carries no access token, so credentials cannot be requested.")
                .source(self)
                .finish(),
            OfferError::MissingPreAuthorizedCodeError => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Missing Pre-Authorized Code")
                .type_url(type_url("holder#missing-pre-authorized-code"))
                .message("The credential offer does not carry a `pre-authorized_code` grant, which is the only grant type currently supported.")
                .source(self)
                .finish(),
            OfferError::MissingCredentialConfigurationsError => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Missing Credential Configurations")
                .type_url(type_url("holder#missing-credential-configurations"))
                .source(self)
                .finish(),
            OfferError::MissingCredentialConfigurationError => ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Missing Credential Configuration")
                .type_url(type_url("holder#missing-credential-configuration"))
                .source(self)
                .finish(),

            // Failures of the remote credential issuer or its authorization server. UniCore acts as a
            // gateway to them here, so the caller is told the upstream failed rather than that we did.
            OfferError::CredentialOfferByReferenceRetrievalError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Credential Offer Retrieval Failed")
                .type_url(type_url("holder#credential-offer-retrieval-failed"))
                .source(self)
                .finish(),
            OfferError::CredentialIssuerMetadataRetrievalError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Credential Issuer Metadata Retrieval Failed")
                .type_url(type_url("holder#credential-issuer-metadata-retrieval-failed"))
                .source(self)
                .finish(),
            OfferError::AuthorizationServerMetadataRetrievalError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Authorization Server Metadata Retrieval Failed")
                .type_url(type_url("holder#authorization-server-metadata-retrieval-failed"))
                .source(self)
                .finish(),
            OfferError::MissingTokenEndpointError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Missing Token Endpoint")
                .type_url(type_url("holder#missing-token-endpoint"))
                .message("The authorization server metadata does not advertise a `token_endpoint`.")
                .source(self)
                .finish(),
            OfferError::TokenResponseError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Token Request Failed")
                .type_url(type_url("holder#token-request-failed"))
                .source(self)
                .finish(),
            OfferError::CredentialResponseError => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Credential Request Failed")
                .type_url(type_url("holder#credential-request-failed"))
                .source(self)
                .finish(),

            // Capabilities UniCore does not implement yet.
            OfferError::UnsupportedDeferredCredentialResponseError => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .title("Deferred Credential Response Unsupported")
                .type_url(type_url("holder#unsupported-deferred-credential-response"))
                .source(self)
                .finish(),
            OfferError::BatchCredentialRequestError => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .title("Batch Credential Request Unsupported")
                .type_url(type_url("holder#unsupported-batch-credential-request"))
                .source(self)
                .finish(),
            OfferError::UnsupportedCredentialFormatError => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .title("Unsupported Credential Format")
                .type_url(type_url("holder#unsupported-credential-format"))
                .source(self)
                .finish(),
        }
    }
}

/// Every variant signals a broken or misconfigured agent rather than a bad request — the holder's own
/// signing key, DID or presentation encoding — so these stay `500`, but carry a problem type that says
/// which step failed.
impl IntoApiErrorExt for PresentationError {
    fn into_api_error(self) -> ApiError {
        match self {
            PresentationError::MissingIdentifierError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Missing Holder Identifier")
                .type_url(type_url("holder#missing-holder-identifier"))
                .source(self)
                .finish(),
            PresentationError::InvalidUrlError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Invalid Holder Identifier URL")
                .type_url(type_url("holder#invalid-holder-identifier-url"))
                .source(self)
                .finish(),
            PresentationError::PresentationBuilderError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Presentation Build Failed")
                .type_url(type_url("holder#presentation-build-failed"))
                .source(self)
                .finish(),
            PresentationError::SerializationError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Presentation Serialization Failed")
                .type_url(type_url("holder#presentation-serialization-failed"))
                .source(self)
                .finish(),
            PresentationError::KeyIdError => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Missing Signing Key Identifier")
                .type_url(type_url("holder#missing-signing-key-identifier"))
                .source(self)
                .finish(),
            PresentationError::SigningError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Presentation Signing Failed")
                .type_url(type_url("holder#presentation-signing-failed"))
                .source(self)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::into_json_value;
    use serde_json::json;

    macro_rules! assert_problem_details {
        ($error:expr, $expected:expr) => {
            assert_eq!(
                into_json_value($error.into_api_error().into_axum_response()).await,
                $expected,
            );
        };
    }

    #[tokio::test]
    async fn credential_errors_successfully_convert_to_problem_details() {
        assert_problem_details!(
            CredentialError::CredentialDecodingError,
            json!({
                "type": type_url("holder#credential-decoding-failed"),
                "title": "Credential Decoding Failed",
                "status": 422,
                "detail": "The supplied credential could not be decoded as a JWT verifiable credential."
            })
        );

        assert_problem_details!(
            CredentialError::InvalidCredentialStatus,
            json!({
                "type": type_url("holder#invalid-credential-status"),
                "title": "Invalid Credential Status",
                "status": 422,
                "detail": "The credential's status could not be verified, or the credential is no longer valid."
            })
        );
    }

    #[tokio::test]
    async fn offer_errors_successfully_convert_to_problem_details() {
        assert_problem_details!(
            OfferError::MissingCredentialOfferError,
            json!({
                "type": type_url("holder#credential-offer-not-found"),
                "title": "Credential Offer Not Found",
                "status": 404,
                "detail": "No credential offer was received under this ID."
            })
        );

        assert_problem_details!(
            OfferError::CredentialOfferStatusNotPendingError,
            json!({
                "type": type_url("holder#credential-offer-not-pending"),
                "title": "Credential Offer Not Pending",
                "status": 409,
                "detail": "The credential offer has already been accepted or rejected."
            })
        );

        assert_problem_details!(
            OfferError::CredentialIssuerMetadataRetrievalError,
            json!({
                "type": type_url("holder#credential-issuer-metadata-retrieval-failed"),
                "title": "Credential Issuer Metadata Retrieval Failed",
                "status": 502,
                "detail": "The Credential Issuer Metadata could not be retrieved"
            })
        );

        assert_problem_details!(
            OfferError::BatchCredentialRequestError,
            json!({
                "type": type_url("holder#unsupported-batch-credential-request"),
                "title": "Batch Credential Request Unsupported",
                "status": 501,
                "detail": "Batch Credential Request are not supported"
            })
        );
    }

    #[tokio::test]
    async fn presentation_errors_successfully_convert_to_problem_details() {
        assert_problem_details!(
            PresentationError::KeyIdError,
            json!({
                "type": type_url("holder#missing-signing-key-identifier"),
                "title": "Missing Signing Key Identifier",
                "status": 500,
                "detail": "Failed to get a key identifier for signing the presentation"
            })
        );

        assert_problem_details!(
            PresentationError::SigningError("no key".to_string()),
            json!({
                "type": type_url("holder#presentation-signing-failed"),
                "title": "Presentation Signing Failed",
                "status": 500,
                "detail": "Failed to sign presentation: no key"
            })
        );
    }
}
