use crate::error::IntoApiErrorExt;
use agent_identity::{
    connection::error::ConnectionError, document::error::DocumentError, service::error::ServiceError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for ConnectionError {
    fn into_api_error(self) -> ApiError {
        match self {}
    }
}

impl IntoApiErrorExt for DocumentError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            // Issues produced during document production indicate internal problems.
            DocumentError::ProduceDocumentError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Missing a document or a DID method is something the client should be made aware of.
            DocumentError::MissingDocumentError => StatusCode::NOT_FOUND,
            DocumentError::MissingDidMethodError => StatusCode::NOT_FOUND,
            // Errors due to adding a service or an invalid DID are usually client errors.
            DocumentError::AddServiceError(_) => StatusCode::BAD_REQUEST,
            DocumentError::InvalidDidError(_) => StatusCode::BAD_REQUEST,
            // Errors when building a verification method suggest that provided data may be malformed.
            DocumentError::VerificationMethodBuilderError(_) => StatusCode::BAD_REQUEST,
            // An unsupported signing algorithm is a feature that isn't implemented.
            DocumentError::UnsupportedSigningAlgorithmError(_) => StatusCode::NOT_IMPLEMENTED,
            // A fixed algorithm is required; if it's missing, the client should fix the request.
            DocumentError::MissingFixedAlgorithmError(_) => StatusCode::BAD_REQUEST,
            // A missing key can be treated as not found.
            DocumentError::MissingKeyError(_) => StatusCode::NOT_FOUND,
            // Failures inserting or generating a DID indicate internal issues.
            DocumentError::VerificationMethodInsertionError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            DocumentError::GenerateDidError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // An invalid node endpoint is typically a client-side configuration error.
            DocumentError::InvalidNodeEndpointError(_) => StatusCode::BAD_REQUEST,
            // Errors coming from IOTA client builder or client are internal failures.
            DocumentError::IotaClientBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            DocumentError::IotaClientError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Missing the required network name is a client error.
            DocumentError::MissingNetworkNameError(_) => StatusCode::BAD_REQUEST,
            // Errors while retrieving the wallet address likely indicate internal problems.
            DocumentError::WalletAddressError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Failure building the alias output is considered an internal error.
            DocumentError::AliasOutputBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Insufficient deposit might be best mapped to Payment Required.
            DocumentError::InsufficientDepositError(_, _) => StatusCode::PAYMENT_REQUIRED,
            // Opaque origins are not supported.
            DocumentError::OpaqueOriginError => StatusCode::NOT_IMPLEMENTED,
            // A host that isn’t a proper domain name is a client error.
            DocumentError::HostError => StatusCode::BAD_REQUEST,
        };

        ApiError::builder(status)
            .title("Document Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for ServiceError {
    fn into_api_error(self) -> ApiError {
        let status = match self {
            // Missing parts of a verification method indicate the client sent incomplete data.
            ServiceError::MissingVerificationMethodFragment(_) => StatusCode::BAD_REQUEST,
            ServiceError::MissingVerificationMethodAlgorithm(_) => StatusCode::BAD_REQUEST,
            // Unsupported algorithms signal a feature not implemented.
            ServiceError::UnsupportedVerificationMethodAlgorithm(_) => StatusCode::NOT_IMPLEMENTED,
            // An empty set of linked DIDs is a client error.
            ServiceError::EmptyLinkedDidsError => StatusCode::BAD_REQUEST,
            // Invalid URL or DID are clearly client errors.
            ServiceError::InvalidUrlError(_) => StatusCode::BAD_REQUEST,
            ServiceError::InvalidDidError(_) => StatusCode::BAD_REQUEST,
            // Errors while building or serializing the Domain Linkage Credential are internal.
            ServiceError::DomainLinkageCredentialBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServiceError::SerializationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Signing errors are treated as server-side failures.
            ServiceError::SigningError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // Invalid timestamp implies the client supplied data in an unexpected format.
            ServiceError::InvalidTimestampError => StatusCode::BAD_REQUEST,
            // An invalid service endpoint indicates a client error.
            ServiceError::InvalidServiceEndpointError(_) => StatusCode::BAD_REQUEST,
            // Errors producing the document or building the service imply internal issues.
            ServiceError::ProduceDocumentError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServiceError::ServiceBuilderError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        ApiError::builder(status)
            .title("Service Error")
            .message(self.to_string())
            .source(self)
            .finish()
    }
}
