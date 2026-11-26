use crate::error::{type_url, IntoApiErrorExt};
use agent_identity::{
    connection::error::ConnectionError,
    document::{
        error::DocumentError,
        key_source::KeySourceError,
        keys::{AddKeyError, DidWebDocumentError, SelectSigningKeyError},
    },
    profile::error::ProfileError,
    service::error::ServiceError,
};
use http_api_problem::ApiError;
use hyper::StatusCode;

/// A problem of type `identity#<anchor>` in `docs/problem-details/identity.md`, detailed by `error`.
fn problem<E: std::error::Error + Send + Sync + 'static>(
    status: StatusCode,
    title: &str,
    anchor: &str,
    error: E,
) -> ApiError {
    ApiError::builder(status)
        .title(title)
        .type_url(type_url(&format!("identity#{anchor}")))
        .message(error.to_string())
        .source(error)
        .finish()
}

/// A problem without a documented type, detailed by `error`.
fn undocumented<E: std::error::Error + Send + Sync + 'static>(status: StatusCode, error: E) -> ApiError {
    ApiError::builder(status)
        .message(error.to_string())
        .source(error)
        .finish()
}

impl IntoApiErrorExt for ConnectionError {
    fn into_api_error(self) -> ApiError {
        use ConnectionError::*;
        match self {
            ConnectionNotFound => ApiError::builder(StatusCode::NOT_FOUND)
                .title("Connection Not Found")
                .type_url(type_url("identity#connection-not-found"))
                .message("No connection found.".to_string())
                .finish(),
            CredentialIssuerMetadataFetchFailed(url) => ApiError::builder(StatusCode::BAD_GATEWAY)
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
            DidWebOnlyError => undocumented(StatusCode::CONFLICT, self),
            NoKeyAlgorithmError(_) => undocumented(StatusCode::BAD_REQUEST, self),
            UnknownVerificationMethodError(_) => undocumented(StatusCode::NOT_FOUND, self),
            NotAnAddedKeyError(_) => problem(StatusCode::CONFLICT, "Not An Added Key", "not-an-added-key", self),
            SelectedSigningKeyError(_) => problem(
                StatusCode::CONFLICT,
                "Selected Signing Key",
                "selected-signing-key",
                self,
            ),
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
        if let ServiceError::SigningError(_) = self {
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "Signing Key Failed",
                "signing-key-failed",
                self,
            );
        }
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

impl IntoApiErrorExt for AddKeyError {
    fn into_api_error(self) -> ApiError {
        match self {
            AddKeyError::Authorization(error) => error.into_api_error(),
            AddKeyError::Persistence(error) => error.into_api_error(),
            AddKeyError::NoDidWebDocument => undocumented(StatusCode::CONFLICT, self),
            AddKeyError::Key(error) => error.into_api_error(),
            AddKeyError::Document(error) => error.into_api_error(),
            AddKeyError::Command(error) => error.into_api_error(),
        }
    }
}

/// The keys `add-key-to-document` adds are keys of the Key Management System, so a missing one is its `key-not-found`.
impl IntoApiErrorExt for KeySourceError {
    fn into_api_error(self) -> ApiError {
        match self {
            KeySourceError::NotFound(_) => {
                crate::v0::kms::error::problem(StatusCode::NOT_FOUND, "Key Not Found", "key-not-found", self)
            }
            KeySourceError::Unusable { .. } => {
                problem(StatusCode::NOT_IMPLEMENTED, "Key Unusable", "key-unusable", self)
            }
            KeySourceError::Unavailable { .. } => {
                problem(StatusCode::BAD_GATEWAY, "Key Unavailable", "key-unavailable", self)
            }
        }
    }
}

impl IntoApiErrorExt for DidWebDocumentError {
    fn into_api_error(self) -> ApiError {
        match self {
            DidWebDocumentError::Persistence(error) => error.into_api_error(),
            DidWebDocumentError::NoDidWebDocument => undocumented(StatusCode::CONFLICT, self),
            DidWebDocumentError::Command(error) => error.into_api_error(),
        }
    }
}

impl IntoApiErrorExt for SelectSigningKeyError {
    fn into_api_error(self) -> ApiError {
        match self {
            SelectSigningKeyError::Authorization(error) => error.into_api_error(),
            SelectSigningKeyError::Persistence(error) => error.into_api_error(),
            SelectSigningKeyError::UnknownDid(_) => undocumented(StatusCode::NOT_FOUND, self),
            SelectSigningKeyError::NotAvailable { .. } => undocumented(StatusCode::CONFLICT, self),
            SelectSigningKeyError::Command(error) => error.into_api_error(),
        }
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
                StatusCode::BAD_GATEWAY,
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
            (DocumentError::DidWebOnlyError, StatusCode::CONFLICT, None),
            (
                DocumentError::NoKeyAlgorithmError(method()),
                StatusCode::BAD_REQUEST,
                None,
            ),
            (
                DocumentError::UnknownVerificationMethodError(method()),
                StatusCode::NOT_FOUND,
                None,
            ),
            (
                DocumentError::NotAnAddedKeyError(method()),
                StatusCode::CONFLICT,
                Some("identity#not-an-added-key"),
            ),
            (
                DocumentError::SelectedSigningKeyError(method()),
                StatusCode::CONFLICT,
                Some("identity#selected-signing-key"),
            ),
        ]);
    }

    fn method() -> identity_kit::did::DidUrl {
        "did:web:example.org#key".parse().unwrap()
    }

    #[test]
    fn add_key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (AddKeyError::NoDidWebDocument, StatusCode::CONFLICT, None),
            (
                AddKeyError::Key(KeySourceError::NotFound("key-id".to_string())),
                StatusCode::NOT_FOUND,
                Some("kms#key-not-found"),
            ),
        ]);
    }

    #[test]
    fn key_source_errors_successfully_convert_to_problem_details() {
        let reason = || "connection refused".to_string();
        assert_problems([
            (
                KeySourceError::Unusable {
                    id: "key-id".to_string(),
                    reason: reason(),
                },
                StatusCode::NOT_IMPLEMENTED,
                Some("identity#key-unusable"),
            ),
            (
                KeySourceError::Unavailable {
                    id: "key-id".to_string(),
                    reason: reason(),
                },
                StatusCode::BAD_GATEWAY,
                Some("identity#key-unavailable"),
            ),
        ]);
    }

    #[test]
    fn a_key_that_cannot_be_had_is_detailed_by_its_id_and_reason() {
        let error = KeySourceError::Unavailable {
            id: "key-id".to_string(),
            reason: "connection refused".to_string(),
        };

        let api_error = error.into_api_error();

        assert_eq!(
            api_error.message(),
            Some("The key `key-id` is not available now: connection refused")
        );
    }

    #[test]
    fn did_web_document_errors_successfully_convert_to_problem_details() {
        assert_problems([(DidWebDocumentError::NoDidWebDocument, StatusCode::CONFLICT, None)]);
    }

    #[test]
    fn select_signing_key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                SelectSigningKeyError::UnknownDid("did:web:example.org".parse().unwrap()),
                StatusCode::NOT_FOUND,
                None,
            ),
            (
                SelectSigningKeyError::NotAvailable {
                    method: method(),
                    reason: "its provider no longer holds it".to_string(),
                },
                StatusCode::CONFLICT,
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
            (
                SigningError(error()),
                StatusCode::SERVICE_UNAVAILABLE,
                Some("identity#signing-key-failed"),
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
