//! The port through which the KMS talks to key providers, and the in-memory provider.

use crate::key_provider::aggregate::KeyProviderType;
use async_trait::async_trait;
pub use identity_kit::keys::InMemoryKeyStore;
use identity_kit::keys::{KeyId, KeySpec, KeyStore, KeyStoreError};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

/// Why a key provider could not be reached. The message comes from the provider and is shown to the operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConnectionError(pub String);

/// Why a provider refuses a key it could otherwise create or adopt. Providers tell this before anything is created.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyRefusal {
    /// The provider can't name a key by this label. The message says why.
    #[error("{0}")]
    InvalidLabel(String),
    /// The provider signs with this algorithm, but not in a way credentials can rely on. The message says why.
    #[error("{0}")]
    AlgorithmRefused(String),
}

/// A key that exists in a key provider, as reported by the provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKey {
    /// The provider's identifier for the key.
    pub id: String,
    /// The provider's full resource name for the key, where it has one (an ARN for AWS KMS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_name: Option<String>,
    /// Whether the provider currently allows the key to be used.
    pub state: ProviderKeyState,
}

/// The keys in a provider that this instance may use.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKeys {
    pub keys: Vec<ProviderKey>,
    /// How many more keys exist in the provider that this instance's credentials may not use. When this is above
    /// zero and `keys` is empty, the credentials may lack a permission rather than the provider being empty.
    pub hidden_keys: usize,
}

/// Whether a key can be used, as reported by its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKeyState {
    Enabled,
    Disabled,
    /// Scheduled for deletion; the provider still holds it until the waiting period ends.
    PendingDeletion,
    /// Any other state in which the key can't be used, such as still being created or imported.
    Unavailable,
}

/// A connection to an external key provider. Keys are generated in, and sign inside, the provider through
/// identity-kit's [`KeyStore`]; this trait adds what an operator needs to set the provider up.
#[async_trait]
pub trait KeyProviderAdapter: KeyStore {
    /// Checks that the provider can be reached with the configured credentials.
    async fn test_connection(&self) -> Result<(), ConnectionError>;

    /// Lists the keys that exist in the provider, that the configured credentials can see and that the provider can
    /// sign with.
    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError>;

    /// Refuses a key the provider would create or hold, but that the KMS must not use, such as a label the provider
    /// can't name a key by. Nothing is asked of the provider, so this is checked before a key is created or adopted.
    ///
    /// # Errors
    ///
    /// The refusal, when the provider refuses the key.
    fn check_spec(&self, spec: &KeySpec) -> Result<(), KeyRefusal>;

    /// Creates a key as `spec` describes, and returns the id that names it for good, unlike a label that can later
    /// name another key. The key is created, and named by its label, even when the returned future is dropped.
    ///
    /// # Errors
    ///
    /// As [`KeyStore::generate`].
    async fn create_key(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError>;

    /// The id that names for good the key `label` names now, or `None` when no usable key has that label.
    ///
    /// # Errors
    ///
    /// Only when the provider can't answer.
    async fn find_key(&self, label: &str) -> Result<Option<KeyId>, KeyStoreError>;
}

/// The adapter for each provider type this instance can use.
pub type KeyProviderAdapters = HashMap<KeyProviderType, Arc<dyn KeyProviderAdapter>>;

/// The in-memory provider keeps keys in this process, so it is always reachable. Its keys are lost on restart.
#[async_trait]
impl KeyProviderAdapter for InMemoryKeyStore {
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        Ok(())
    }

    /// Every key in the store was generated through the KMS, which already knows it, so none are listed.
    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
        Ok(ProviderKeys::default())
    }

    fn check_spec(&self, _spec: &KeySpec) -> Result<(), KeyRefusal> {
        Ok(())
    }

    /// A key's id is its label, or its thumbprint without one, and never names another key.
    async fn create_key(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        self.generate(spec).await
    }

    async fn find_key(&self, label: &str) -> Result<Option<KeyId>, KeyStoreError> {
        let key = KeyId::new(label);
        if self.exists(&key).await? {
            Ok(Some(key))
        } else {
            Ok(None)
        }
    }
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test_utils {
    use super::{ConnectionError, KeyProviderAdapter, KeyRefusal, ProviderKeys};
    use async_trait::async_trait;
    use identity_kit::{
        jose::{Jwk, JwsAlgorithm},
        keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError},
    };

    /// A provider that can't be reached: every request fails with "connection refused".
    pub struct UnreachableKeyProvider;

    fn refused() -> ConnectionError {
        ConnectionError("connection refused".to_string())
    }

    #[async_trait]
    impl KeyStore for UnreachableKeyProvider {
        fn capabilities(&self) -> KeyStoreCapabilities {
            KeyStoreCapabilities::new([JwsAlgorithm::ES256])
                .with_generate()
                .remote()
        }

        async fn public_jwk(&self, _key: &KeyId) -> Result<Jwk, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }

        async fn sign(&self, _key: &KeyId, _alg: JwsAlgorithm, _data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }

        async fn exists(&self, _key: &KeyId) -> Result<bool, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }

        async fn generate(&self, _spec: KeySpec) -> Result<KeyId, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }
    }

    #[async_trait]
    impl KeyProviderAdapter for UnreachableKeyProvider {
        async fn test_connection(&self) -> Result<(), ConnectionError> {
            Err(refused())
        }

        async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
            Err(refused())
        }

        fn check_spec(&self, _spec: &KeySpec) -> Result<(), KeyRefusal> {
            Ok(())
        }

        async fn create_key(&self, _spec: KeySpec) -> Result<KeyId, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }

        async fn find_key(&self, _label: &str) -> Result<Option<KeyId>, KeyStoreError> {
            Err(KeyStoreError::backend(refused()))
        }
    }
}
