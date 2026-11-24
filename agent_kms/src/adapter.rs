//! The port through which the KMS talks to key providers, how adapters are built from a provider's settings, and the
//! in-memory provider.

use crate::key_provider::{aggregate::KeyProviderType, settings::KeyProviderSettings};
use async_trait::async_trait;
use identity_kit::jose::{Jwk, JwsAlgorithm};
pub use identity_kit::keys::InMemoryKeyStore;
use identity_kit::keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tracing::warn;

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

    /// Whether `a` and `b` name one key, such as two versions of it. Only equal ids do, unless the provider names a
    /// key in more than one way for good.
    fn same_key(&self, a: &KeyId, b: &KeyId) -> bool {
        a == b
    }
}

/// Builds the adapter of a provider type whose providers are added with [`KeyProviderSettings`], such as STACKIT KMS,
/// whose settings say where its keys are and who this instance signs in as.
pub trait KeyProviderFactory: Send + Sync {
    /// An adapter for a provider with `settings`. It is built on first use and then kept, so building does no I/O.
    ///
    /// # Errors
    ///
    /// Why no adapter can be built. It is shown to the operator as the reason the provider can't be reached.
    fn build(&self, settings: &KeyProviderSettings) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError>;
}

/// The adapters of the provider types this instance can use: one adapter for a type without settings, and one for
/// each settings of a type with a [`KeyProviderFactory`], built on first use and kept from then on.
#[derive(Default)]
pub struct KeyProviderAdapters {
    fixed: HashMap<KeyProviderType, Arc<dyn KeyProviderAdapter>>,
    factories: HashMap<KeyProviderType, Arc<dyn KeyProviderFactory>>,
    built: Mutex<HashMap<KeyProviderSettings, Arc<dyn KeyProviderAdapter>>>,
}

impl KeyProviderAdapters {
    /// These adapters, with `factory` building those of `provider_type` from the settings of its providers.
    #[must_use]
    pub fn with_factory(mut self, provider_type: KeyProviderType, factory: impl KeyProviderFactory + 'static) -> Self {
        self.factories.insert(provider_type, Arc::new(factory));
        self
    }

    /// The adapter for a provider of `provider_type` with `settings`, or `None` when this instance has none for the
    /// type.
    ///
    /// An adapter that can't be built, also for want of settings, is an [`UnreachableKeyProvider`] that tells why. It
    /// is not kept, so the next use tries again.
    ///
    /// # Panics
    ///
    /// When a factory panicked while building an adapter before.
    pub fn get(
        &self,
        provider_type: KeyProviderType,
        settings: Option<&KeyProviderSettings>,
    ) -> Option<Arc<dyn KeyProviderAdapter>> {
        if let Some(adapter) = self.fixed.get(&provider_type) {
            return Some(adapter.clone());
        }
        let factory = self.factories.get(&provider_type)?;
        let Some(settings) = settings else {
            return Some(unbuilt(
                provider_type,
                ConnectionError(format!("A `{provider_type}` key provider needs its settings")),
            ));
        };

        let mut built = self.built.lock().expect("lock poisoned");
        if let Some(adapter) = built.get(settings) {
            return Some(adapter.clone());
        }
        match factory.build(settings) {
            Ok(adapter) => {
                built.insert(settings.clone(), adapter.clone());
                Some(adapter)
            }
            Err(error) => Some(unbuilt(provider_type, error)),
        }
    }
}

/// Stands in for an adapter of `provider_type` that couldn't be built, failing every request with `error`.
fn unbuilt(provider_type: KeyProviderType, error: ConnectionError) -> Arc<dyn KeyProviderAdapter> {
    warn!(%provider_type, %error, "Key provider adapter could not be built");
    Arc::new(UnreachableKeyProvider(error.0))
}

/// Every adapter given is used for every provider of its type, which takes no settings.
impl<const N: usize> From<[(KeyProviderType, Arc<dyn KeyProviderAdapter>); N]> for KeyProviderAdapters {
    fn from(fixed: [(KeyProviderType, Arc<dyn KeyProviderAdapter>); N]) -> Self {
        Self {
            fixed: fixed.into(),
            ..Self::default()
        }
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

/// A provider that can't be reached: every request fails with its reason, such as why its adapter couldn't be built.
pub struct UnreachableKeyProvider(
    /// Why the provider can't be reached, shown to the operator.
    pub String,
);

impl UnreachableKeyProvider {
    fn error(&self) -> ConnectionError {
        ConnectionError(self.0.clone())
    }
}

#[async_trait]
impl KeyStore for UnreachableKeyProvider {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new([JwsAlgorithm::ES256])
            .with_generate()
            .with_delete()
            .remote()
    }

    async fn public_jwk(&self, _key: &KeyId) -> Result<Jwk, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }

    async fn sign(&self, _key: &KeyId, _alg: JwsAlgorithm, _data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }

    async fn exists(&self, _key: &KeyId) -> Result<bool, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }

    async fn generate(&self, _spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }

    async fn delete(&self, _key: &KeyId) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }
}

#[async_trait]
impl KeyProviderAdapter for UnreachableKeyProvider {
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        Err(self.error())
    }

    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
        Err(self.error())
    }

    fn check_spec(&self, _spec: &KeySpec) -> Result<(), KeyRefusal> {
        Ok(())
    }

    async fn create_key(&self, _spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }

    async fn find_key(&self, _label: &str) -> Result<Option<KeyId>, KeyStoreError> {
        Err(KeyStoreError::backend(self.error()))
    }
}

#[cfg(any(test, feature = "test_utils"))]
pub mod test_utils {
    use super::{ConnectionError, InMemoryKeyStore, KeyProviderAdapter, KeyProviderFactory};
    use crate::key_provider::settings::KeyProviderSettings;
    use std::sync::{Arc, Mutex};

    /// Builds an in-memory provider for each settings, and records the settings of every build it is asked for. Fails
    /// every build when `failure` is set, with it as the reason.
    #[derive(Clone, Default)]
    pub struct InMemoryFactory {
        pub builds: Arc<Mutex<Vec<KeyProviderSettings>>>,
        pub failure: Option<String>,
    }

    impl KeyProviderFactory for InMemoryFactory {
        fn build(&self, settings: &KeyProviderSettings) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError> {
            self.builds.lock().unwrap().push(settings.clone());
            if let Some(failure) = &self.failure {
                return Err(ConnectionError(failure.clone()));
            }
            Ok(Arc::new(InMemoryKeyStore::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_utils::InMemoryFactory;
    use super::*;
    use crate::key_provider::settings::tests::stackit;

    fn adapters(factory: &InMemoryFactory) -> KeyProviderAdapters {
        KeyProviderAdapters::default().with_factory(KeyProviderType::StackitKms, factory.clone())
    }

    #[test]
    fn reuses_the_adapter_it_built() {
        let factory = InMemoryFactory::default();
        let adapters = adapters(&factory);
        adapters.get(KeyProviderType::StackitKms, Some(&stackit("key-ring")));

        adapters.get(KeyProviderType::StackitKms, Some(&stackit("key-ring")));

        assert_eq!(*factory.builds.lock().unwrap(), [stackit("key-ring")]);
    }

    #[test]
    fn builds_another_adapter_for_other_settings() {
        let factory = InMemoryFactory::default();
        let adapters = adapters(&factory);
        adapters.get(KeyProviderType::StackitKms, Some(&stackit("key-ring")));

        adapters.get(KeyProviderType::StackitKms, Some(&stackit("other-key-ring")));

        assert_eq!(
            *factory.builds.lock().unwrap(),
            [stackit("key-ring"), stackit("other-key-ring")]
        );
    }

    #[test]
    fn tries_again_to_build_an_adapter_that_could_not_be_built() {
        let factory = InMemoryFactory {
            failure: Some("no workload identity token".to_string()),
            ..InMemoryFactory::default()
        };
        let adapters = adapters(&factory);
        adapters.get(KeyProviderType::StackitKms, Some(&stackit("key-ring")));

        adapters.get(KeyProviderType::StackitKms, Some(&stackit("key-ring")));

        assert_eq!(
            *factory.builds.lock().unwrap(),
            [stackit("key-ring"), stackit("key-ring")]
        );
    }

    #[tokio::test]
    async fn builds_no_adapter_without_settings() {
        let adapters = adapters(&InMemoryFactory::default());

        let adapter = adapters.get(KeyProviderType::StackitKms, None).unwrap();

        assert_eq!(
            adapter.test_connection().await.unwrap_err().to_string(),
            "A `stackit_kms` key provider needs its settings"
        );
    }
}
