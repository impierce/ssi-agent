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
            ServiceError::EmptyPresentationIds => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "No Presentation IDs Given",
                "empty-presentation-ids",
            ),
            ServiceError::PresentationNotFound(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Presentation Not Found",
                "presentation-not-found",
            ),
            ServiceError::PresentationInvalid(_, _) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Presentation Invalid",
                "presentation-invalid",
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::assert_problems;
    use agent_identity::service::lifecycle::ServiceManagementError;
    use agent_shared::handlers::CommandHandlerError;
    use cqrs_es::AggregateError;
    use shared_kernel::authorization::AuthorizationError;

    #[test]
    fn connection_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                ConnectionError::ConnectionNotFound,
                StatusCode::NOT_FOUND,
                Some("identity#connection-not-found"),
            ),
            (
                ConnectionError::CredentialIssuerMetadataFetchFailed("https://example.com".parse().unwrap()),
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("identity#credential-issuer-metadata-fetch-failed"),
            ),
            (
                ConnectionError::MissingDomain("connection-1".to_string()),
                StatusCode::BAD_REQUEST,
                Some("identity#missing-domain"),
            ),
        ]);
    }

    #[test]
    fn document_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                DocumentError::OpaqueOriginError,
                StatusCode::BAD_REQUEST,
                Some("identity#opaque-origin"),
            ),
            (
                DocumentError::HostError,
                StatusCode::BAD_REQUEST,
                Some("identity#invalid-host"),
            ),
            (
                DocumentError::InvalidOriginError("origin".to_string()),
                StatusCode::BAD_REQUEST,
                Some("identity#invalid-origin"),
            ),
            (
                DocumentError::MissingDocumentError,
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
            (
                DocumentError::IotaPublishDocumentError("error".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ]);
    }

    #[test]
    fn profile_errors_successfully_convert_to_problem_details() {
        assert_problems([(
            ProfileError::ConfigurationConflict,
            StatusCode::CONFLICT,
            Some("conflict#resource-provisioned-by-configuration"),
        )]);
    }

    #[test]
    fn every_service_error_maps_to_a_problem_type() {
        use ServiceError::*;

        let error = || "error".to_string();
        assert_problems([
            (
                AlreadyExists,
                StatusCode::CONFLICT,
                Some("identity#service-already-exists"),
            ),
            (NotFound, StatusCode::NOT_FOUND, Some("identity#service-not-found")),
            (
                EmptyLinkedDidsError,
                StatusCode::BAD_REQUEST,
                Some("identity#no-linked-dids"),
            ),
            (EmptyOriginsError, StatusCode::BAD_REQUEST, Some("identity#no-origins")),
            (
                EmptyPresentationIds,
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("identity#empty-presentation-ids"),
            ),
            (
                PresentationNotFound(error()),
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("identity#presentation-not-found"),
            ),
            (
                PresentationInvalid(error(), error()),
                StatusCode::UNPROCESSABLE_ENTITY,
                Some("identity#presentation-invalid"),
            ),
        ]);

        assert_problems(
            [
                MissingVerificationMethodFragment(error()),
                MissingVerificationMethodAlgorithm(error()),
                UnsupportedVerificationMethodAlgorithm(error()),
                InvalidUrlError(error()),
                InvalidDidError(error()),
                DomainLinkageCredentialBuilderError(error()),
                SerializationError(error()),
                SigningError(error()),
                InvalidTimestampError,
                InvalidServiceEndpointError(error()),
                ProduceDocumentError(error()),
                ServiceBuilderError(error()),
            ]
            .map(|error| {
                (
                    error,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Some("identity#service-operation-failed"),
                )
            }),
        );
    }

    #[test]
    fn service_management_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                ServiceManagementError::Authorization(AuthorizationError::Forbidden),
                StatusCode::FORBIDDEN,
                Some("authorization#forbidden"),
            ),
            (
                ServiceManagementError::Command(CommandHandlerError::Aggregate(AggregateError::UserError(
                    ServiceError::NotFound,
                ))),
                StatusCode::NOT_FOUND,
                Some("identity#service-not-found"),
            ),
            (
                ServiceManagementError::Infrastructure(anyhow::anyhow!("database unavailable")),
                StatusCode::INTERNAL_SERVER_ERROR,
                None,
            ),
        ]);
    }
}
