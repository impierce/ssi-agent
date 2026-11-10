use crate::error::{type_url, IntoApiErrorExt};
use agent_kms::key_provider::error::KeyProviderError;
use agent_kms::state::TestConnectionError;
use http_api_problem::ApiError;
use hyper::StatusCode;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::assert_problems;
    use agent_kms::key_provider::aggregate::KeyProviderType;

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
}
