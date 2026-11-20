//! The port through which the KMS talks to key providers, and the in-memory provider.

use crate::key_provider::{aggregate::KeyProviderType, settings::KeyProviderSettings};
use async_trait::async_trait;
pub use identity_kit::keys::InMemoryKeyStore;
use identity_kit::{
    jose::JwsAlgorithm,
    keys::{KeyId, KeySpec, KeyStore, KeyStoreError},
};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

/// Why a key provider could not be reached. The message comes from the provider and is shown to the operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ConnectionError(pub String);

/// Why a provider can't give a key this label. The message is shown to the operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct InvalidLabel(pub String);

/// Why a provider refuses to create or adopt keys for an algorithm it can sign with. The message is shown to the
/// operator.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct RefusedAlgorithm(pub String);

/// What signs with the keys of this instance, outside the Key Management System, such as DID documents.
#[async_trait]
pub trait KeyUsage: Send + Sync {
    /// The verification method whose key is the key `key_id`, with the public key `public_key`, if any.
    async fn verification_method(
        &self,
        key_id: &str,
        public_key: &identity_kit::jose::Jwk,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>>;
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

    /// Checks, before anything is created, that the provider can give a key `label`.
    ///
    /// # Errors
    ///
    /// Fails when the provider can't use `label` to name a key.
    fn check_label(&self, _label: &str) -> Result<(), InvalidLabel> {
        Ok(())
    }

    /// Checks, before anything is created or adopted, that keys of this provider may sign with `algorithm`.
    ///
    /// # Errors
    ///
    /// Fails when the provider can sign with `algorithm`, but not in a way credentials can rely on.
    fn check_algorithm(&self, _algorithm: JwsAlgorithm) -> Result<(), RefusedAlgorithm> {
        Ok(())
    }

    /// Generates a key as [`KeyStore::generate`] does, but returns an id that names this key for good, also where
    /// the provider lets a label be moved to another key. A labelled key can still be deleted by its label.
    ///
    /// # Errors
    ///
    /// Fails as [`KeyStore::generate`] does.
    async fn create(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        self.generate(spec).await
    }

    /// Returns the id that names the key labelled `label` for good, as [`create`](Self::create) does, to record a key
    /// that exists in the provider but that this instance didn't record.
    ///
    /// By default a label is the key's id.
    ///
    /// # Errors
    ///
    /// Fails with [`KeyStoreError::NotFound`] when no key has the label, or as [`KeyStore::exists`] does.
    async fn adopt(&self, label: &str) -> Result<KeyId, KeyStoreError> {
        let key = KeyId::new(label);
        if self.exists(&key).await? {
            Ok(key)
        } else {
            Err(KeyStoreError::NotFound(key))
        }
    }

    /// Deletes the key [`create`](Self::create) or [`adopt`](Self::adopt) returned `created` for, and frees `label`,
    /// which names it, only while the label still names that key.
    ///
    /// By default the key is deleted by `created`, for providers whose label goes with the key.
    ///
    /// Whether the recorded key `recorded` and the key `key` are one key of the provider, such as two of its versions.
    fn same_key(&self, recorded: &str, key: &KeyId) -> bool {
        recorded == key.as_str()
    }

    /// # Errors
    ///
    /// Fails as [`KeyStore::delete`] does.
    async fn discard(&self, created: &KeyId, _label: Option<&str>) -> Result<(), KeyStoreError> {
        self.delete(created).await
    }
}

/// Builds the adapter of a provider from the settings it was added with. Providers are added at runtime, so their
/// adapters can only be built once their settings are known.
pub trait KeyProviderFactory: Send + Sync {
    /// Builds the adapter for a provider added with `settings`, which match the factory's provider type.
    ///
    /// # Errors
    ///
    /// Fails when the provider can't be reached with these settings in this environment, for example because a
    /// workload identity token it needs isn't available.
    fn build(&self, settings: Option<&KeyProviderSettings>) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError>;
}

/// The factory for each provider type this instance can use.
pub type KeyProviderFactories = HashMap<KeyProviderType, Arc<dyn KeyProviderFactory>>;

/// A factory for a provider type that takes no settings, which always hands out the same adapter.
pub struct FixedAdapter(pub Arc<dyn KeyProviderAdapter>);

impl KeyProviderFactory for FixedAdapter {
    fn build(&self, _settings: Option<&KeyProviderSettings>) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError> {
        Ok(self.0.clone())
    }
}

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
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test_utils {
    use super::{ConnectionError, KeyProviderAdapter, ProviderKeys};
    use async_trait::async_trait;
    use identity_kit::{
        jose::{Jwk, JwsAlgorithm},
        keys::{KeyId, KeyStore, KeyStoreCapabilities, KeyStoreError},
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

        async fn generate(&self, _spec: identity_kit::keys::KeySpec) -> Result<KeyId, KeyStoreError> {
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
    }
}
