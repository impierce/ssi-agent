use agent_shared::config::config;
use async_trait::async_trait;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::storage::{JwkStorage, KeyStorageError, KeyStorageErrorKind};
use identity_kit::{
    jose::{Jwk, JwsAlgorithm},
    keys::{KeyId, KeyStore, KeyStoreCapabilities, KeyStoreError},
};
use std::collections::HashMap;

/// IdentityKit's [`KeyStore`] over the keys in the Stronghold snapshot.
///
/// A Stronghold record holds the raw private key, which doesn't tell which algorithm it is for, so the store is
/// given the algorithm of every key it serves. Other records in the snapshot are not found.
#[derive(Clone, Debug)]
pub struct StrongholdKeyStore {
    storage: StrongholdExtStorage,
    keys: HashMap<KeyId, JwsAlgorithm>,
}

impl StrongholdKeyStore {
    /// The algorithms the store signs with.
    const ALGORITHMS: [JwsAlgorithm; 2] = [JwsAlgorithm::EdDSA, JwsAlgorithm::ES256];

    /// A store over the `keys` of `storage`, each with its algorithm.
    pub fn new(storage: StrongholdExtStorage, keys: impl IntoIterator<Item = (KeyId, JwsAlgorithm)>) -> Self {
        Self {
            storage,
            keys: keys.into_iter().collect(),
        }
    }

    /// A store over the configured issuer keys.
    pub fn issuer_keys(storage: StrongholdExtStorage) -> Self {
        Self::new(storage, issuer_keys())
    }

    fn algorithm(&self, key: &KeyId) -> Result<JwsAlgorithm, KeyStoreError> {
        self.keys
            .get(key)
            .copied()
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))
    }

    /// The public key of `key` as the Stronghold storage returns it, with `alg` and the thumbprint as `kid`.
    async fn stronghold_jwk(
        &self,
        key: &KeyId,
        alg: JwsAlgorithm,
    ) -> Result<identity_iota::verification::jwk::Jwk, KeyStoreError> {
        let key_id = identity_iota::storage::KeyId::new(key.as_str());
        match alg {
            JwsAlgorithm::EdDSA => self.storage.get_ed25519_public_key(&key_id).await,
            JwsAlgorithm::ES256 => self.storage.get_es256_public_key(&key_id).await,
            alg => return Err(KeyStoreError::UnsupportedAlgorithm(alg)),
        }
        .map_err(|error| storage_error(key, error))
    }
}

/// The configured issuer keys with their algorithms.
pub fn issuer_keys() -> [(KeyId, JwsAlgorithm); 2] {
    let secret_manager = &config().secret_manager;

    [
        (
            KeyId::new(secret_manager.issuer_eddsa_key_id.as_str()),
            JwsAlgorithm::EdDSA,
        ),
        (
            KeyId::new(secret_manager.issuer_es256_key_id.as_str()),
            JwsAlgorithm::ES256,
        ),
    ]
}

fn storage_error(key: &KeyId, error: KeyStorageError) -> KeyStoreError {
    match error.kind() {
        KeyStorageErrorKind::KeyNotFound => KeyStoreError::NotFound(key.clone()),
        _ => KeyStoreError::Backend(Box::new(error)),
    }
}

#[async_trait]
impl KeyStore for StrongholdKeyStore {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new(Self::ALGORITHMS)
    }

    async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
        let jwk = self.stronghold_jwk(key, self.algorithm(key)?).await?;

        serde_json::to_value(jwk)
            .and_then(serde_json::from_value)
            .map_err(|error| KeyStoreError::InvalidKey {
                key: key.clone(),
                reason: "the public key is not a JWK".to_string(),
                source: Some(Box::new(error)),
            })
    }

    async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        let key_alg = self.algorithm(key)?;
        if key_alg != alg {
            return Err(KeyStoreError::AlgorithmMismatch {
                key: key.clone(),
                key_alg,
                requested: alg,
            });
        }
        let public_key = self.stronghold_jwk(key, alg).await?;

        self.storage
            .sign(&identity_iota::storage::KeyId::new(key.as_str()), data, &public_key)
            .await
            .map_err(|error| storage_error(key, error))
    }

    async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        let Ok(alg) = self.algorithm(key) else {
            return Ok(false);
        };

        match self.stronghold_jwk(key, alg).await {
            Ok(_) => Ok(true),
            Err(KeyStoreError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stronghold_storage;
    use identity_kit::keys::testsuite;

    #[tokio::test]
    async fn passes_identity_kit_key_store_testsuite_with_the_issuer_keys() {
        let store = StrongholdKeyStore::issuer_keys(stronghold_storage().await);

        testsuite::run_with_keys(&store, &issuer_keys()).await;
    }

    #[tokio::test]
    async fn does_not_serve_records_it_has_no_algorithm_for() {
        let store = StrongholdKeyStore::new(
            stronghold_storage().await,
            [(
                KeyId::new(config().secret_manager.issuer_eddsa_key_id.as_str()),
                JwsAlgorithm::EdDSA,
            )],
        );
        let es256_key = KeyId::new(config().secret_manager.issuer_es256_key_id.as_str());

        assert!(!store.exists(&es256_key).await.unwrap());
        assert!(matches!(
            store.public_jwk(&es256_key).await,
            Err(KeyStoreError::NotFound(key)) if key == es256_key
        ));
    }
}
