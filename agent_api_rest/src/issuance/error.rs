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
            UnsupportedCredentialFormat => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-format"
                ))
                .title("Unsupported Credential Format")
                .source(self)
                .finish(),
            UnsupportedCredentialType => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#unsupported-credential-type"
                ))
                .title("Unsupported Credential Type")
                .source(self)
                .finish(),
            InvalidCredentialSubjectError(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-credential-subject"
                ))
                .title("Invalid Credential Subject")
                .source(self)
                .finish(),
            InvalidIdentifierError => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-identifier"
                ))
                .title("Invalid Identifier")
                .source(self)
                .finish(),
            MissingCredentialDataError => todo!("specification API?"),
            InvalidExpirationDateError => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details/issuance#invalid-expiration-date"
                ))
                .title("Invalid Expiration Date")
                .source(self)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for OfferError {
    fn into_api_error(self) -> ApiError {
        use OfferError::*;

        match self {
            MissingCredentialOfferError => ApiError::builder(StatusCode::BAD_REQUEST)
                .type_url(format!("{DOCUMENTATION_URL}problem-details#missing-credential-offer"))
                .title("Missing Credential Offer")
                .source(self)
                .finish(),
            SendCredentialOfferError(_) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .type_url(format!(
                    "{DOCUMENTATION_URL}problem-details#send-credential-offer-error"
                ))
                .title("Send Credential Offer Error")
                .source(self)
                .finish(),
            MissingCredentialError => todo!("specification API?"),
            MissingProofError => todo!("specification API?"),
            InvalidProofError(_) => todo!("specification API?"),
            MissingProofIssuerError => todo!("specification API?"),
            UnsupportedTokenRequestGrantTypeError => todo!("specification API?"),
            InvalidCredentialOfferUriError(_) => todo!("can never happen?"),
        }
    }
}

impl IntoApiErrorExt for ServerConfigError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}
