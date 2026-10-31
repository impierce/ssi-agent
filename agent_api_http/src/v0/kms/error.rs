use crate::error::{type_url, IntoApiErrorExt};
use agent_kms::key::error::KeyError;
use agent_kms::key_provider::error::KeyProviderError;
use agent_kms::state::{CreateKeyError, TestConnectionError};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_kit::keys::KeyStoreError;

impl IntoApiErrorExt for KeyProviderError {
    fn into_api_error(self) -> ApiError {
        let (status, title, problem_type) = match &self {
            KeyProviderError::AlreadyAdded(_) => (
                StatusCode::CONFLICT,
                "Key Provider Already Added",
                "kms#key-provider-already-added",
            ),
            KeyProviderError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                "Key Provider Not Found",
                "kms#key-provider-not-found",
            ),
            KeyProviderError::DevelopmentOnly(_) => (
                StatusCode::BAD_REQUEST,
                "Key Provider Not Available",
                "kms#key-provider-development-only",
            ),
            KeyProviderError::MissingName => (
                StatusCode::BAD_REQUEST,
                "Key Provider Name Missing",
                "kms#missing-key-provider-name",
            ),
            KeyProviderError::NameTaken(_) => (
                StatusCode::CONFLICT,
                "Key Provider Name Taken",
                "kms#key-provider-name-taken",
            ),
        };

        ApiError::builder(status)
            .title(title)
            .type_url(type_url(problem_type))
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for TestConnectionError {
    fn into_api_error(self) -> ApiError {
        match self {
            TestConnectionError::Authorization(error) => error.into_api_error(),
            TestConnectionError::Query(error) => error.into_api_error(),
            TestConnectionError::NotFound(provider_type) => KeyProviderError::NotFound(provider_type).into_api_error(),
            TestConnectionError::Unsupported(_) => ApiError::builder(StatusCode::NOT_IMPLEMENTED)
                .title("Key Provider Not Supported")
                .type_url(type_url("kms#key-provider-not-supported"))
                .message(self.to_string())
                .source(self)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for KeyError {
    fn into_api_error(self) -> ApiError {
        let (status, title, problem_type) = match &self {
            KeyError::AlreadyRecorded => (StatusCode::CONFLICT, "Key Already Recorded", "kms#key-already-recorded"),
        };

        ApiError::builder(status)
            .title(title)
            .type_url(type_url(problem_type))
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for KeyStoreError {
    fn into_api_error(self) -> ApiError {
        let (status, title, problem_type) = match &self {
            KeyStoreError::AlreadyExists(_) => (StatusCode::CONFLICT, "Key Already Exists", "kms#key-already-exists"),
            KeyStoreError::Unsupported(_)
            | KeyStoreError::UnsupportedAlgorithm(_)
            | KeyStoreError::UnsupportedKeyType(_)
            | KeyStoreError::InvalidSpec { .. } => (
                StatusCode::BAD_REQUEST,
                "Key Not Supported By Provider",
                "kms#key-not-supported-by-provider",
            ),
            // Anything else is the provider failing a request that should have worked.
            _ => (StatusCode::BAD_GATEWAY, "Key Provider Error", "kms#key-provider-error"),
        };

        ApiError::builder(status)
            .title(title)
            .type_url(type_url(problem_type))
            .message(self.to_string())
            .source(self)
            .finish()
    }
}

impl IntoApiErrorExt for CreateKeyError {
    fn into_api_error(self) -> ApiError {
        match self {
            CreateKeyError::Provider(error) => error.into_api_error(),
            CreateKeyError::KeyStore(error) => error.into_api_error(),
            CreateKeyError::Record(error) => error.into_api_error(),
            CreateKeyError::UnsupportedAlgorithm(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Key Algorithm Not Supported")
                .type_url(type_url("kms#unsupported-key-algorithm"))
                .message(self.to_string())
                .source(self)
                .finish(),
            CreateKeyError::InvalidLabel(_) => ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Key Label")
                .type_url(type_url("kms#invalid-key-label"))
                .message(self.to_string())
                .source(self)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::assert_problems;
    use agent_kms::adapter::InvalidLabel;
    use agent_kms::key_provider::aggregate::KeyProviderType;
    use identity_kit::{
        jose::{JwsAlgorithm, KeyType},
        keys::KeyId,
    };

    #[test]
    fn key_provider_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                KeyProviderError::AlreadyAdded(KeyProviderType::AwsKmsAmbient),
                StatusCode::CONFLICT,
                Some("kms#key-provider-already-added"),
            ),
            (
                KeyProviderError::NotFound(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_FOUND,
                Some("kms#key-provider-not-found"),
            ),
            (
                KeyProviderError::DevelopmentOnly(KeyProviderType::InMemory),
                StatusCode::BAD_REQUEST,
                Some("kms#key-provider-development-only"),
            ),
            (
                KeyProviderError::MissingName,
                StatusCode::BAD_REQUEST,
                Some("kms#missing-key-provider-name"),
            ),
            (
                KeyProviderError::NameTaken("Keys".to_string()),
                StatusCode::CONFLICT,
                Some("kms#key-provider-name-taken"),
            ),
        ]);
    }

    #[test]
    fn connection_test_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                TestConnectionError::NotFound(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_FOUND,
                Some("kms#key-provider-not-found"),
            ),
            (
                TestConnectionError::Unsupported(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_IMPLEMENTED,
                Some("kms#key-provider-not-supported"),
            ),
        ]);
    }

    #[test]
    fn key_errors_successfully_convert_to_problem_details() {
        assert_problems([(
            KeyError::AlreadyRecorded,
            StatusCode::CONFLICT,
            Some("kms#key-already-recorded"),
        )]);
    }

    #[test]
    fn key_store_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                KeyStoreError::AlreadyExists(KeyId::new("issuer")),
                StatusCode::CONFLICT,
                Some("kms#key-already-exists"),
            ),
            (
                KeyStoreError::UnsupportedKeyType(KeyType::Rsa2048),
                StatusCode::BAD_REQUEST,
                Some("kms#key-not-supported-by-provider"),
            ),
            (
                KeyStoreError::NotFound(KeyId::new("issuer")),
                StatusCode::BAD_GATEWAY,
                Some("kms#key-provider-error"),
            ),
        ]);
    }

    #[test]
    fn create_key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                CreateKeyError::UnsupportedAlgorithm(JwsAlgorithm::ES384),
                StatusCode::BAD_REQUEST,
                Some("kms#unsupported-key-algorithm"),
            ),
            (
                CreateKeyError::InvalidLabel(InvalidLabel("`aws/x` can't be an AWS KMS alias name".to_string())),
                StatusCode::BAD_REQUEST,
                Some("kms#invalid-key-label"),
            ),
        ]);
    }
}
