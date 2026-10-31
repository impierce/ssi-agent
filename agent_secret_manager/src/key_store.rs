use agent_shared::config::config;
use async_trait::async_trait;
use identity_kit::{
    jose::{Jwk, JwsAlgorithm, KeyType},
    keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError},
};
use iota_stronghold::{
    procedures::{Ed25519Sign, GenerateKey, KeyType as StrongholdKeyType, PublicKey, StrongholdProcedure},
    Client, ClientError, KeyProvider, Location, SnapshotPath, Stronghold,
};
use std::{collections::HashMap, path::Path, sync::Arc};
use stronghold_ext::{
    execute_procedure_ext,
    procs::{
        es256::{self, Es256Procs},
        rs256::{self, Rs256Procs},
    },
};
use tokio::sync::Mutex;

/// The Stronghold client that holds the keys.
const CLIENT_PATH: &[u8] = b"iota_identity_client";
/// The vault in which every key is a record named by its id.
const VAULT_PATH: &[u8] = b"iota_identity_vault";
/// Prefix of the client store entries that record the algorithm of each generated key.
const ALGORITHM_PREFIX: &str = "algorithm/";

/// IdentityKit's [`KeyStore`] over the keys in a Stronghold snapshot.
///
/// A key is a record in the `iota_identity_vault` vault of the `iota_identity_client` client, named by its id, the
/// layout of the snapshots the agent has always written. A record holds only the private key, so the algorithm of a
/// key this store generates is kept in the client store next to it. The algorithms of older keys are given when the
/// store is opened, and the fixed ids keys used to be generated under are known as well: `ed25519-0` (`EdDSA`),
/// `es256-0` (`ES256`) and `rs256-0` (`RS256`).
#[derive(Clone)]
pub struct StrongholdKeyStore {
    stronghold: Stronghold,
    client: Client,
    snapshot_path: SnapshotPath,
    key_provider: Arc<KeyProvider>,
    /// The algorithms of keys whose algorithm isn't recorded in the snapshot.
    known_keys: Arc<HashMap<KeyId, JwsAlgorithm>>,
    /// Serializes generating a key with writing the snapshot.
    writes: Arc<Mutex<()>>,
}

impl std::fmt::Debug for StrongholdKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrongholdKeyStore")
            .field("snapshot_path", &self.snapshot_path.as_path())
            .finish_non_exhaustive()
    }
}

impl StrongholdKeyStore {
    /// The algorithms the store signs with.
    const ALGORITHMS: [JwsAlgorithm; 3] = [JwsAlgorithm::EdDSA, JwsAlgorithm::ES256, JwsAlgorithm::RS256];

    /// Opens the snapshot at `path`, encrypted with a key derived from `password`, and creates it when there is none.
    /// `known_keys` gives the algorithms of keys the snapshot doesn't record one for.
    ///
    /// # Errors
    ///
    /// [`KeyStoreError::Backend`] when the snapshot can't be read, for example with a wrong password.
    pub fn open(
        path: impl AsRef<Path>,
        password: &str,
        known_keys: impl IntoIterator<Item = (KeyId, JwsAlgorithm)>,
    ) -> Result<Self, KeyStoreError> {
        let snapshot_path = SnapshotPath::from_path(path);
        let key_provider = KeyProvider::with_passphrase_hashed_blake2b(password.as_bytes().to_vec())
            .map_err(|error| KeyStoreError::Backend(error.to_string().into()))?;
        let stronghold = Stronghold::default();

        if snapshot_path.exists() {
            stronghold
                .load_snapshot(&key_provider, &snapshot_path)
                .map_err(KeyStoreError::backend)?;
        }
        let client = match stronghold.load_client(CLIENT_PATH) {
            Ok(client) => client,
            Err(ClientError::ClientDataNotPresent) => {
                stronghold.create_client(CLIENT_PATH).map_err(KeyStoreError::backend)?
            }
            Err(error) => return Err(KeyStoreError::backend(error)),
        };

        let store = Self {
            stronghold,
            client,
            snapshot_path,
            key_provider: Arc::new(key_provider),
            known_keys: Arc::new(known_keys.into_iter().collect()),
            writes: Arc::new(Mutex::new(())),
        };
        if !store.snapshot_path.exists() {
            store.persist()?;
        }

        Ok(store)
    }

    fn location(key: &KeyId) -> Location {
        Location::generic(VAULT_PATH.to_vec(), key.as_str().as_bytes().to_vec())
    }

    fn algorithm_entry(key: &KeyId) -> Vec<u8> {
        format!("{ALGORITHM_PREFIX}{key}").into_bytes()
    }

    /// The algorithm of `key`, `None` when the store doesn't know it.
    fn algorithm(&self, key: &KeyId) -> Result<Option<JwsAlgorithm>, KeyStoreError> {
        let recorded = self
            .client
            .store()
            .get(&Self::algorithm_entry(key))
            .map_err(KeyStoreError::backend)?;
        if let Some(recorded) = recorded {
            let alg = String::from_utf8(recorded)
                .ok()
                .and_then(|alg| alg.parse().ok())
                .ok_or_else(|| KeyStoreError::invalid_key(key.clone(), "its recorded algorithm is unknown"))?;
            return Ok(Some(alg));
        }

        Ok(self.known_keys.get(key).copied().or(match key.as_str() {
            "ed25519-0" => Some(JwsAlgorithm::EdDSA),
            "es256-0" => Some(JwsAlgorithm::ES256),
            "rs256-0" => Some(JwsAlgorithm::RS256),
            _ => None,
        }))
    }

    /// The algorithm of `key`, which must exist.
    fn existing_algorithm(&self, key: &KeyId) -> Result<JwsAlgorithm, KeyStoreError> {
        match self.algorithm(key)? {
            Some(alg) if self.has_record(key)? => Ok(alg),
            _ => Err(KeyStoreError::NotFound(key.clone())),
        }
    }

    fn has_record(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        self.client
            .record_exists(&Self::location(key))
            .map_err(KeyStoreError::backend)
    }

    /// Writes the client into the snapshot file.
    fn persist(&self) -> Result<(), KeyStoreError> {
        self.stronghold
            .write_client(CLIENT_PATH)
            .map_err(KeyStoreError::backend)?;
        self.stronghold
            .commit_with_keyprovider(&self.snapshot_path, &self.key_provider)
            .map_err(KeyStoreError::backend)
    }

    /// The public key of `key`, which signs with `alg`, without `alg` and `kid`.
    fn public_key(&self, key: &KeyId, alg: JwsAlgorithm) -> Result<Jwk, KeyStoreError> {
        let private_key = Self::location(key);
        let invalid = |error: identity_kit::jose::JwkError| KeyStoreError::InvalidKey {
            key: key.clone(),
            reason: "its public key is malformed".to_string(),
            source: Some(Box::new(error)),
        };

        match alg {
            JwsAlgorithm::EdDSA => {
                let public_key: Vec<u8> = self
                    .client
                    .execute_procedure(StrongholdProcedure::PublicKey(PublicKey {
                        ty: StrongholdKeyType::Ed25519,
                        private_key,
                    }))
                    .map_err(KeyStoreError::backend)?
                    .into();
                Jwk::from_ed25519_public(&public_key).map_err(invalid)
            }
            JwsAlgorithm::ES256 => {
                let public_key: Vec<u8> =
                    execute_procedure_ext(&self.client, Es256Procs::PublicKey(es256::PublicKey { private_key }))
                        .map_err(KeyStoreError::backend)?
                        .into();
                Jwk::from_p256_public(&public_key).map_err(invalid)
            }
            JwsAlgorithm::RS256 => {
                let public_key: Vec<u8> =
                    execute_procedure_ext(&self.client, Rs256Procs::PublicKey(rs256::PublicKey { private_key }))
                        .map_err(KeyStoreError::backend)?
                        .into();
                Jwk::from_pkcs1_der(&public_key).map_err(invalid)
            }
            alg => Err(KeyStoreError::UnsupportedAlgorithm(alg)),
        }
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

/// The key type a key that signs with `alg` is generated as.
pub(crate) fn key_type(alg: JwsAlgorithm) -> Option<KeyType> {
    match alg {
        JwsAlgorithm::EdDSA => Some(KeyType::Ed25519),
        JwsAlgorithm::ES256 => Some(KeyType::P256),
        JwsAlgorithm::RS256 => Some(KeyType::Rsa2048),
        _ => None,
    }
}

#[async_trait]
impl KeyStore for StrongholdKeyStore {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new(Self::ALGORITHMS).with_generate()
    }

    /// The public key of `key`, with its `alg` and its RFC 7638 thumbprint as `kid`.
    async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
        let alg = self.existing_algorithm(key)?;
        let jwk = self.public_key(key, alg)?;
        let kid = jwk.thumbprint_sha256().map_err(|error| KeyStoreError::InvalidKey {
            key: key.clone(),
            reason: "its public key has no thumbprint".to_string(),
            source: Some(Box::new(error)),
        })?;

        Ok(jwk.with_alg(alg.as_str()).with_kid(kid))
    }

    async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        let key_alg = self.existing_algorithm(key)?;
        if key_alg != alg {
            return Err(KeyStoreError::AlgorithmMismatch {
                key: key.clone(),
                key_alg,
                requested: alg,
            });
        }
        let private_key = Self::location(key);
        let msg = data.to_vec();

        let signature: Vec<u8> = match alg {
            JwsAlgorithm::EdDSA => self
                .client
                .execute_procedure(Ed25519Sign { private_key, msg })
                .map_err(KeyStoreError::backend)?
                .to_vec(),
            JwsAlgorithm::ES256 => {
                execute_procedure_ext(&self.client, Es256Procs::Sign(es256::Sign { private_key, msg }))
                    .map_err(KeyStoreError::backend)?
                    .into()
            }
            JwsAlgorithm::RS256 => {
                execute_procedure_ext(&self.client, Rs256Procs::Sign(rs256::Sign { private_key, msg }))
                    .map_err(KeyStoreError::backend)?
                    .into()
            }
            alg => return Err(KeyStoreError::UnsupportedAlgorithm(alg)),
        };

        Ok(signature)
    }

    async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        Ok(self.algorithm(key)?.is_some() && self.has_record(key)?)
    }

    /// Generates a key under its label, or under a random id without one, and writes it into the snapshot.
    async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        spec.validate()?;
        if !Self::ALGORITHMS.contains(&spec.alg) {
            return Err(KeyStoreError::UnsupportedAlgorithm(spec.alg));
        }
        if key_type(spec.alg) != Some(spec.key_type) {
            return Err(KeyStoreError::UnsupportedKeyType(spec.key_type));
        }

        let _write = self.writes.lock().await;
        let key = KeyId::new(spec.label.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()));
        if self.has_record(&key)? {
            return Err(KeyStoreError::AlreadyExists(key));
        }
        let output = Self::location(&key);

        match spec.alg {
            JwsAlgorithm::EdDSA => {
                self.client
                    .execute_procedure(StrongholdProcedure::GenerateKey(GenerateKey {
                        ty: StrongholdKeyType::Ed25519,
                        output,
                    }))
                    .map_err(KeyStoreError::backend)?;
            }
            JwsAlgorithm::ES256 => {
                execute_procedure_ext(&self.client, Es256Procs::GenerateKey(es256::GenerateKey { output }))
                    .map_err(KeyStoreError::backend)?;
            }
            JwsAlgorithm::RS256 => {
                execute_procedure_ext(&self.client, Rs256Procs::GenerateKey(rs256::GenerateKey { output }))
                    .map_err(KeyStoreError::backend)?;
            }
            alg => return Err(KeyStoreError::UnsupportedAlgorithm(alg)),
        }
        self.client
            .store()
            .insert(Self::algorithm_entry(&key), spec.alg.as_str().as_bytes().to_vec(), None)
            .map_err(KeyStoreError::backend)?;
        self.persist()?;

        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stronghold_key_store;
    use identity_kit::{
        jose::{DefaultVerifier, JwsVerifier as _},
        keys::testsuite,
    };
    use serde_json::json;

    /// A path for a snapshot of its own, removed when dropped.
    struct TempSnapshot(std::path::PathBuf);

    impl TempSnapshot {
        fn new() -> Self {
            iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();
            Self(std::env::temp_dir().join(format!("stronghold-key-store-{}.dat", uuid::Uuid::new_v4())))
        }
    }

    impl Drop for TempSnapshot {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The existing issuer keys of the test snapshot, and an `RS256` key next to them in a copy of it.
    #[tokio::test]
    async fn passes_identity_kit_key_store_testsuite_with_the_issuer_keys() {
        let snapshot = TempSnapshot::new();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/res/test.stronghold.dat"),
            &snapshot.0,
        )
        .unwrap();
        let store =
            StrongholdKeyStore::open(&snapshot.0, &config().secret_manager.stronghold_password, issuer_keys()).unwrap();
        let rs256_key = store
            .generate(KeySpec::new(KeyType::Rsa2048, JwsAlgorithm::RS256))
            .await
            .unwrap();
        let mut keys = issuer_keys().to_vec();
        keys.push((rs256_key, JwsAlgorithm::RS256));

        testsuite::run_with_keys(&store, &keys).await;
    }

    #[tokio::test]
    async fn passes_identity_kit_key_store_testsuite_with_generated_keys() {
        let snapshot = TempSnapshot::new();
        let store = StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();

        testsuite::run(&store).await;
    }

    /// The public keys of the test snapshot, as the storage the agent used before reported them.
    #[tokio::test]
    async fn serves_the_keys_of_existing_snapshots_unchanged() {
        let store = stronghold_key_store().await;
        let [(eddsa_key, _), (es256_key, _)] = issuer_keys();

        assert_eq!(
            serde_json::to_value(store.public_jwk(&eddsa_key).await.unwrap()).unwrap(),
            json!({
                "kty": "OKP",
                "crv": "Ed25519",
                "x": "GlnK9ePs802XxAglROQzoGurm9Qpv0IFPEbdMCILN_U",
                "alg": "EdDSA",
                "kid": "bQKQRzaop7CgEvqVq8UlgLGsdF-R-hnLFkKFZqW2VN0"
            })
        );
        assert_eq!(
            serde_json::to_value(store.public_jwk(&es256_key).await.unwrap()).unwrap(),
            json!({
                "kty": "EC",
                "crv": "P-256",
                "x": "Fmk13gO2SGLbuXeL24qJPHCNncnI6lBu6ZQL2EVZv4E",
                "y": "fz2KvMhufzUpMeL9-K2re9fwA3mzg1bpfbceIQSuihY",
                "alg": "ES256",
                "kid": "oOY2dMVU7GK5al1q7EAxuoYloxMQlv5ZNZOatiUXQHg"
            })
        );
    }

    #[tokio::test]
    async fn keeps_generated_keys_and_their_algorithms_across_reopening() {
        let snapshot = TempSnapshot::new();
        let store = StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();
        let key = store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("signing-key"))
            .await
            .unwrap();
        let public_jwk = store.public_jwk(&key).await.unwrap();
        drop(store);

        let reopened = StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();
        let signature = reopened.sign(&key, JwsAlgorithm::ES256, b"data").await.unwrap();

        assert_eq!(reopened.public_jwk(&key).await.unwrap(), public_jwk);
        DefaultVerifier
            .verify(JwsAlgorithm::ES256, b"data", &signature, &public_jwk)
            .unwrap();
    }

    #[tokio::test]
    async fn knows_the_algorithms_of_keys_generated_under_the_fixed_ids() {
        let snapshot = TempSnapshot::new();
        let store = StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();
        let key = KeyId::new("rs256-0");
        // Generated the way the agent's previous storage did: a record without a recorded algorithm.
        execute_procedure_ext(
            &store.client,
            Rs256Procs::GenerateKey(rs256::GenerateKey {
                output: StrongholdKeyStore::location(&key),
            }),
        )
        .unwrap();

        assert!(store.exists(&key).await.unwrap());
        assert_eq!(store.public_jwk(&key).await.unwrap().alg(), Some("RS256"));
    }

    #[tokio::test]
    async fn does_not_serve_records_it_knows_no_algorithm_for() {
        let snapshot = TempSnapshot::new();
        let store = StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();
        let key = KeyId::new("unknown");
        store
            .client
            .execute_procedure(StrongholdProcedure::GenerateKey(GenerateKey {
                ty: StrongholdKeyType::Ed25519,
                output: StrongholdKeyStore::location(&key),
            }))
            .unwrap();

        assert!(!store.exists(&key).await.unwrap());
        assert!(matches!(store.public_jwk(&key).await, Err(KeyStoreError::NotFound(missing)) if missing == key));
    }

    #[tokio::test]
    async fn refuses_a_snapshot_with_the_wrong_password() {
        let snapshot = TempSnapshot::new();
        StrongholdKeyStore::open(&snapshot.0, "password", []).unwrap();

        assert!(matches!(
            StrongholdKeyStore::open(&snapshot.0, "another password", []),
            Err(KeyStoreError::Backend(_))
        ));
    }
}
