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
            ConnectionNotFound => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Connection Not Found")
                .type_url(type_url("identity#connection-not-found"))
                .message("No connection found.".to_string())
                .finish(),
            CredentialIssuerMetadataFetchFailed(url) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Credential Issuer Metadata Fetch Failed")
                .type_url(type_url("identity#credential-issuer-metadata-fetch-failed"))
                .message(format!("Failed to fetch credential issuer metadata from: {url}"))
                .finish(),
            MissingDomain(connection_id) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Missing Domain")
                .type_url(type_url("identity#missing-domain"))
                .message(format!("Connection with id '{connection_id}' is missing a domain"))
                .finish(),
            DIDResolutionFailed(url) => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("DID Configurations could not be resolved")
                .type_url(type_url("identity#did-config-failed"))
                .message(format!("Failed to resolve DID Configurations from '{url}'"))
                .finish(),
        }
    }
}

impl IntoApiErrorExt for DocumentError {
    fn into_api_error(self) -> ApiError {
        use DocumentError::*;

        match self {
            OpaqueOriginError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Opaque Origin Not Supported")
                .type_url(type_url("identity#opaque-origin"))
                .message(self.to_string())
                .finish(),
            HostError => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Host Must Be A Domain Name")
                .type_url(type_url("identity#invalid-host"))
                .message(self.to_string())
                .finish(),
            InvalidOriginError(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Domain Or Origin")
                .type_url(type_url("identity#invalid-origin"))
                .message(self.to_string())
                .finish(),
            // TODO: Implement appropriate Problem Details responses
            _ => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR),
        }
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
        let (status, title, kind) = match &self {
            ServiceError::AlreadyExists => (StatusCode::CONFLICT, "Service already exists", "service-already-exists"),
            ServiceError::NotFound => (StatusCode::NOT_FOUND, "Service not found", "service-not-found"),
            ServiceError::EmptyLinkedDidsError => {
                (StatusCode::BAD_REQUEST, "No eligible signing DID", "no-linked-dids")
            }
            ServiceError::EmptyOriginsError => (StatusCode::BAD_REQUEST, "No Origins Given", "no-origins"),
            // TODO: Implement appropriate Problem Details responses
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Service operation failed",
                "service-operation-failed",
            ),
        };
        ApiError::builder(status)
            .title(title)
            .type_url(type_url(&format!("identity#{kind}")))
            .message(self.to_string())
            .finish()
    }
}

impl IntoApiErrorExt for agent_identity::service::lifecycle::ServiceManagementError {
    fn into_api_error(self) -> ApiError {
        use agent_identity::service::lifecycle::ServiceManagementError::*;
        match self {
            Authorization(error) => error.into_api_error(),
            Command(error) => error.into_api_error(),
            Infrastructure(error) => {
                tracing::error!("Service management failed: {error:#}");
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}
