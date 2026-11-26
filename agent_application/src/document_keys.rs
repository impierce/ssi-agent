//! The identity's `did:web` document and the Key Management System, wired to each other through their ports: the
//! document adds and signs with the KMS's keys, and the KMS asks the document before it deletes one.

use agent_identity::document::key_source::{KeySource, KeySourceError};
use agent_identity::document::keys::verification_method_of_key;
use agent_identity::state::IdentityState;
use agent_kms::key::service::{signing_key, SigningKeyError};
use agent_kms::key::usage::KeyUsage;
use agent_kms::KmsState;
use async_trait::async_trait;
use identity_kit::keys::{KeySigner, KeyStore};
use shared_kernel::cqrs_es::persist::PersistenceError;
use std::sync::Arc;

/// The keys of the Key Management System, as the keys the `did:web` document adds.
pub(crate) struct KmsKeySource(pub Arc<KmsState>);

#[async_trait]
impl KeySource for KmsKeySource {
    async fn key(&self, id: &str) -> Result<KeySigner<Arc<dyn KeyStore>>, KeySourceError> {
        signing_key(&self.0, id)
            .await
            .map_err(|error| key_source_error(id, error))
    }
}

/// Why the KMS can't give the key `id`, by whether asking again can help: a provider that doesn't answer, or a record
/// that can't be read, may later.
fn key_source_error(id: &str, error: SigningKeyError) -> KeySourceError {
    let id = id.to_string();
    match error {
        SigningKeyError::NotFound(_) => KeySourceError::NotFound(id),
        SigningKeyError::Unsupported(_) | SigningKeyError::Unusable(_) => KeySourceError::Unusable {
            id,
            reason: error.to_string(),
        },
        SigningKeyError::Provider(_) | SigningKeyError::Persistence(_) => KeySourceError::Unavailable {
            id,
            reason: error.to_string(),
        },
    }
}

/// The identity's documents, as what uses the KMS's keys: a key is used by the verification method it was added as.
pub(crate) struct DocumentKeyUsage(pub Arc<IdentityState>);

#[async_trait]
impl KeyUsage for DocumentKeyUsage {
    async fn user(&self, id: &str) -> Result<Option<String>, PersistenceError> {
        let method = verification_method_of_key(&self.0, id).await?;
        Ok(method.map(|method| method.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_kms::key_provider::aggregate::KeyProviderType;
    use identity_kit::keys::KeyStoreError;

    fn failure() -> std::io::Error {
        std::io::Error::other("connection refused")
    }

    #[test]
    fn tells_kms_errors_apart_by_whether_asking_again_can_help() {
        let errors = [
            SigningKeyError::NotFound("key-id".to_string()),
            SigningKeyError::Unsupported(KeyProviderType::AwsKmsAmbient),
            SigningKeyError::Unusable(KeyStoreError::backend(failure())),
            SigningKeyError::Provider(KeyStoreError::backend(failure())),
            SigningKeyError::Persistence(PersistenceError::UnknownError(Box::new(failure()))),
        ];

        let kinds: Vec<&str> = errors
            .into_iter()
            .map(|error| match key_source_error("key-id", error) {
                KeySourceError::NotFound(_) => "not found",
                KeySourceError::Unusable { .. } => "unusable",
                KeySourceError::Unavailable { .. } => "unavailable",
            })
            .collect();

        assert_eq!(
            kinds,
            ["not found", "unusable", "unusable", "unavailable", "unavailable"]
        );
    }
}
