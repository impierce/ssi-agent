use crate::error::{type_url, IntoApiErrorExt};
use agent_identity::{
    connection::error::ConnectionError, document::error::DocumentError, profile::error::ProfileError,
    service::error::ServiceError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

impl IntoApiErrorExt for ConnectionError {
    fn into_api_error(self) -> ApiError {
        use ConnectionError::*;
        match self {
            ConnectionNotFound(connection_id) => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Connection Not Found")
                .type_url(type_url("not-found#connection-not-found"))
                .message(format!("No connection found with id: {connection_id}"))
                .finish(),
            ConnectionAlreadyExists(connection_id) => ApiError::builder(StatusCode::CONFLICT)
                .title("Connection Already Exists")
                .type_url(type_url("conflict#connection-already-exists"))
                .message(format!("A connection with id '{connection_id}' already exists"))
                .finish(),
            ConnectionSyncFailed(connection_id) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Connection Synchronization Failed")
                .type_url(type_url("internal-server-error#connection-sync-failed"))
                .message(format!(
                    "Failed to synchronize latest connection with id: {connection_id}"
                ))
                .finish(),
            MissingCredentialOfferEndpoint => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Credential Offer Endpoint")
                .type_url(type_url("bad-request#missing-credential-offer-endpoint"))
                .message("Connection does not have a credential offer endpoint configured")
                .finish(),
            CredentialIssuerMetadataFetchFailed(url) => ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Credential Issuer Metadata Fetch Failed")
                .type_url(type_url("bad-gateway#credential-issuer-metadata-fetch-failed"))
                .message(format!("Failed to fetch credential issuer metadata from: {url}"))
                .finish(),
            DIDWebResolutionFailed(url) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("DID Web Resolution Failed")
                .type_url(type_url("bad-gateway#did-web-resolution-failed"))
                .message(format!("Failed to resolve DID Web for: {url}"))
                .finish(),
            MissingDomain(connection_id) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Domain")
                .type_url(type_url("bad-request#missing-domain"))
                .message(format!("Connection with id '{connection_id}' is missing a domain"))
                .finish(),
            DIDConfigurationResolutionFailed(url) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("DID Configurations could not be resolved")
                .type_url(type_url("bad-request#did-config-failed"))
                .message(format!("Failed to resolve DID Configurations from '{url}'"))
                .finish(),
        }
    }
}

impl IntoApiErrorExt for DocumentError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl IntoApiErrorExt for ProfileError {
    fn into_api_error(self) -> ApiError {
        use ProfileError::*;

        match self {
            ConfigurationConflict => ApiError::builder(StatusCode::CONFLICT)
                .title("Resource Provisioned by Configuration")
                .type_url(type_url("conflict#resource-provisioned-by-configuration"))
                .message("This resource was provisioned and cannot be modified during runtime")
                .finish(),
        }
    }
}

impl IntoApiErrorExt for ServiceError {
    fn into_api_error(self) -> ApiError {
        // TODO: Implement appropriate Problem Details responses
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
