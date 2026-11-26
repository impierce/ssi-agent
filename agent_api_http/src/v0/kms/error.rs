use crate::error::{type_url, IntoApiErrorExt};
use agent_kms::adapter::KeyRefusal;
use agent_kms::key::{
    error::KeyError,
    service::{DeleteKeyError, RecordKeyError},
};
use agent_kms::key_provider::error::KeyProviderError;
use agent_kms::state::{KeyProviderAccessError, RemoveKeyProviderError};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_kit::keys::KeyStoreError;

/// A problem of type `kms#<anchor>` in `docs/problem-details/kms.md`, detailed by `error`.
pub(crate) fn problem<E: std::error::Error + Send + Sync + 'static>(
    status: StatusCode,
    title: &str,
    anchor: &str,
    error: E,
) -> ApiError {
    ApiError::builder(status)
        .title(title)
        .type_url(type_url(&format!("kms#{anchor}")))
        .message(error.to_string())
        .source(error)
        .finish()
}

impl IntoApiErrorExt for KeyProviderError {
    fn into_api_error(self) -> ApiError {
        let (status, title, anchor) = match &self {
            KeyProviderError::AlreadyAdded(_) => (
                StatusCode::CONFLICT,
                "Key Provider Already Added",
                "key-provider-already-added",
            ),
            KeyProviderError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                "Key Provider Not Found",
                "key-provider-not-found",
            ),
            KeyProviderError::DevelopmentOnly(_) => (
                StatusCode::BAD_REQUEST,
                "Key Provider Not Available",
                "key-provider-development-only",
            ),
            KeyProviderError::MissingName => (
                StatusCode::BAD_REQUEST,
                "Key Provider Name Missing",
                "missing-key-provider-name",
            ),
            KeyProviderError::NameTaken(_) => (
                StatusCode::CONFLICT,
                "Key Provider Name Taken",
                "key-provider-name-taken",
            ),
        };

        problem(status, title, anchor, self)
    }
}

impl IntoApiErrorExt for KeyProviderAccessError {
    fn into_api_error(self) -> ApiError {
        match self {
            KeyProviderAccessError::Authorization(error) => error.into_api_error(),
            KeyProviderAccessError::Query(error) => error.into_api_error(),
            KeyProviderAccessError::NotFound(provider_type) => {
                KeyProviderError::NotFound(provider_type).into_api_error()
            }
            KeyProviderAccessError::Unsupported(_) => problem(
                StatusCode::NOT_IMPLEMENTED,
                "Key Provider Not Supported",
                "key-provider-not-supported",
                self,
            ),
        }
    }
}

impl IntoApiErrorExt for RemoveKeyProviderError {
    fn into_api_error(self) -> ApiError {
        match self {
            RemoveKeyProviderError::Authorization(error) => error.into_api_error(),
            RemoveKeyProviderError::Persistence(error) => error.into_api_error(),
            RemoveKeyProviderError::HasKeys(_) => problem(
                StatusCode::CONFLICT,
                "Key Provider Has Keys",
                "key-provider-has-keys",
                self,
            ),
            RemoveKeyProviderError::Command(error) => error.into_api_error(),
        }
    }
}

/// What a provider answered. Its refusals of the request are client errors; anything else failed at the provider.
impl IntoApiErrorExt for KeyStoreError {
    fn into_api_error(self) -> ApiError {
        let (status, title, anchor) = match &self {
            KeyStoreError::NotFound(_) => (StatusCode::NOT_FOUND, "Key Not Found", "key-not-found"),
            KeyStoreError::AlreadyExists(_) => (StatusCode::CONFLICT, "Key Already Exists", "key-already-exists"),
            KeyStoreError::Unsupported(_)
            | KeyStoreError::UnsupportedAlgorithm(_)
            | KeyStoreError::UnsupportedKeyType(_)
            | KeyStoreError::InvalidSpec { .. } => (
                StatusCode::BAD_REQUEST,
                "Key Not Supported By Provider",
                "key-not-supported-by-provider",
            ),
            KeyStoreError::InvalidKey { .. } => (StatusCode::BAD_REQUEST, "Unsupported Key", "unsupported-key"),
            _ => (StatusCode::BAD_GATEWAY, "Key Provider Error", "key-provider-error"),
        };

        problem(status, title, anchor, self)
    }
}

impl IntoApiErrorExt for KeyRefusal {
    fn into_api_error(self) -> ApiError {
        let (title, anchor) = match &self {
            KeyRefusal::InvalidLabel(_) => ("Invalid Key Label", "invalid-key-label"),
            KeyRefusal::AlgorithmRefused(_) => {
                ("Key Algorithm Refused By Provider", "key-algorithm-refused-by-provider")
            }
        };

        problem(StatusCode::BAD_REQUEST, title, anchor, self)
    }
}

impl IntoApiErrorExt for KeyError {
    fn into_api_error(self) -> ApiError {
        let (status, title, anchor) = match &self {
            KeyError::AlreadyRecorded => (StatusCode::CONFLICT, "Key Already Recorded", "key-already-recorded"),
            KeyError::NotFound => (StatusCode::NOT_FOUND, "Key Not Found", "key-not-found"),
        };

        problem(status, title, anchor, self)
    }
}

impl IntoApiErrorExt for RecordKeyError {
    fn into_api_error(self) -> ApiError {
        match self {
            RecordKeyError::Authorization(error) => error.into_api_error(),
            RecordKeyError::Access(error) => error.into_api_error(),
            RecordKeyError::Persistence(error) => error.into_api_error(),
            RecordKeyError::UnsupportedAlgorithm(_) => problem(
                StatusCode::BAD_REQUEST,
                "Unsupported Key Algorithm",
                "unsupported-key-algorithm",
                self,
            ),
            RecordKeyError::UnsupportedKey(_) => {
                problem(StatusCode::BAD_REQUEST, "Unsupported Key", "unsupported-key", self)
            }
            RecordKeyError::Refused(error) => error.into_api_error(),
            RecordKeyError::Provider(error) => error.into_api_error(),
            RecordKeyError::LabelNotFound(_) => problem(StatusCode::NOT_FOUND, "Key Not Found", "key-not-found", self),
            RecordKeyError::AlreadyRecorded(_) => problem(
                StatusCode::CONFLICT,
                "Key Already Recorded",
                "key-already-recorded",
                self,
            ),
            RecordKeyError::Record(error) => error.into_api_error(),
            // The key is left in the provider unrecorded; the message names it, so an operator can adopt or delete it.
            RecordKeyError::Abandoned { .. } => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Unexpected Error")
                .type_url(type_url("unexpected#unexpected-error"))
                .message(self.to_string())
                .source(self)
                .finish(),
        }
    }
}

impl IntoApiErrorExt for DeleteKeyError {
    fn into_api_error(self) -> ApiError {
        match self {
            DeleteKeyError::Authorization(error) => error.into_api_error(),
            DeleteKeyError::Persistence(error) => error.into_api_error(),
            DeleteKeyError::NotFound(_) => problem(StatusCode::NOT_FOUND, "Key Not Found", "key-not-found", self),
            DeleteKeyError::InUse { .. } => problem(StatusCode::CONFLICT, "Key In Use", "key-in-use", self),
            DeleteKeyError::Unsupported(provider_type) => {
                KeyProviderAccessError::Unsupported(provider_type).into_api_error()
            }
            DeleteKeyError::Provider(error) => error.into_api_error(),
            DeleteKeyError::Record(error) => error.into_api_error(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::tests::assert_problems;
    use agent_kms::key_provider::aggregate::KeyProviderType;
    use identity_kit::jose::{JwsAlgorithm, KeyType};
    use identity_kit::keys::KeyId;

    fn provider_failure() -> KeyStoreError {
        KeyStoreError::backend(std::io::Error::other("throttled"))
    }

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
    fn key_provider_access_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                KeyProviderAccessError::NotFound(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_FOUND,
                Some("kms#key-provider-not-found"),
            ),
            (
                KeyProviderAccessError::Unsupported(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_IMPLEMENTED,
                Some("kms#key-provider-not-supported"),
            ),
        ]);
    }

    #[test]
    fn remove_key_provider_errors_successfully_convert_to_problem_details() {
        assert_problems([(
            RemoveKeyProviderError::HasKeys(KeyProviderType::AwsKmsAmbient),
            StatusCode::CONFLICT,
            Some("kms#key-provider-has-keys"),
        )]);
    }

    #[test]
    fn key_store_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                KeyStoreError::NotFound(KeyId::new("issuer")),
                StatusCode::NOT_FOUND,
                Some("kms#key-not-found"),
            ),
            (
                KeyStoreError::AlreadyExists(KeyId::new("issuer")),
                StatusCode::CONFLICT,
                Some("kms#key-already-exists"),
            ),
            (
                KeyStoreError::Unsupported("generate"),
                StatusCode::BAD_REQUEST,
                Some("kms#key-not-supported-by-provider"),
            ),
            (
                KeyStoreError::UnsupportedAlgorithm(JwsAlgorithm::ES384),
                StatusCode::BAD_REQUEST,
                Some("kms#key-not-supported-by-provider"),
            ),
            (
                KeyStoreError::UnsupportedKeyType(KeyType::Rsa2048),
                StatusCode::BAD_REQUEST,
                Some("kms#key-not-supported-by-provider"),
            ),
            (
                KeyStoreError::InvalidSpec {
                    key_type: KeyType::P256,
                    alg: JwsAlgorithm::EdDSA,
                },
                StatusCode::BAD_REQUEST,
                Some("kms#key-not-supported-by-provider"),
            ),
            (
                KeyStoreError::invalid_key(KeyId::new("rsa"), "an RSA key"),
                StatusCode::BAD_REQUEST,
                Some("kms#unsupported-key"),
            ),
            (
                provider_failure(),
                StatusCode::BAD_GATEWAY,
                Some("kms#key-provider-error"),
            ),
        ]);
    }

    #[test]
    fn key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                KeyError::AlreadyRecorded,
                StatusCode::CONFLICT,
                Some("kms#key-already-recorded"),
            ),
            (KeyError::NotFound, StatusCode::NOT_FOUND, Some("kms#key-not-found")),
        ]);
    }

    #[test]
    fn record_key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                RecordKeyError::UnsupportedAlgorithm(JwsAlgorithm::ES384),
                StatusCode::BAD_REQUEST,
                Some("kms#unsupported-key-algorithm"),
            ),
            (
                RecordKeyError::UnsupportedKey("issuer".to_string()),
                StatusCode::BAD_REQUEST,
                Some("kms#unsupported-key"),
            ),
            (
                RecordKeyError::Refused(KeyRefusal::InvalidLabel("no spaces".to_string())),
                StatusCode::BAD_REQUEST,
                Some("kms#invalid-key-label"),
            ),
            (
                RecordKeyError::Refused(KeyRefusal::AlgorithmRefused("4096 bytes at most".to_string())),
                StatusCode::BAD_REQUEST,
                Some("kms#key-algorithm-refused-by-provider"),
            ),
            (
                RecordKeyError::LabelNotFound("issuer".to_string()),
                StatusCode::NOT_FOUND,
                Some("kms#key-not-found"),
            ),
            (
                RecordKeyError::AlreadyRecorded("issuer".to_string()),
                StatusCode::CONFLICT,
                Some("kms#key-already-recorded"),
            ),
            (
                RecordKeyError::Abandoned {
                    key: KeyId::new("issuer"),
                    cause: Box::new(RecordKeyError::Provider(provider_failure())),
                    deletion: provider_failure(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("unexpected#unexpected-error"),
            ),
        ]);
    }

    #[test]
    fn delete_key_errors_successfully_convert_to_problem_details() {
        assert_problems([
            (
                DeleteKeyError::NotFound("key-id".to_string()),
                StatusCode::NOT_FOUND,
                Some("kms#key-not-found"),
            ),
            (
                DeleteKeyError::InUse {
                    id: "key-id".to_string(),
                    user: "did:web:example.org#key".to_string(),
                },
                StatusCode::CONFLICT,
                Some("kms#key-in-use"),
            ),
            (
                DeleteKeyError::Unsupported(KeyProviderType::AwsKmsAmbient),
                StatusCode::NOT_IMPLEMENTED,
                Some("kms#key-provider-not-supported"),
            ),
        ]);
    }
}
