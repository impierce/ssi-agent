use agent_shared::config::{config, SupportedDidMethod};
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::{
    did::DIDUrl,
    storage::{JwkStorage, KeyId, KeyType},
    verification::{jwk::Jwk, jws::JwsAlgorithm},
};
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use jsonwebtoken::Algorithm;
use log::info;
use std::collections::HashMap;

pub mod service;
pub mod subject;

pub struct StrongholdManager {
    pub stronghold_storage: StrongholdExtStorage,
    pub verification_method_ids: HashMap<StorageKey, DIDUrl>,
}

impl StrongholdManager {
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        Self {
            stronghold_storage,
            verification_method_ids: HashMap::new(),
        }
    }

    pub async fn get_public_key(&self, key_id: KeyId, algorithm: &Algorithm) -> anyhow::Result<Jwk> {
        match algorithm {
            Algorithm::EdDSA => self.stronghold_storage.get_ed25519_public_key(&key_id).await,
            Algorithm::ES256 => self.stronghold_storage.get_es256_public_key(&key_id).await,
            _ => anyhow::bail!("Unsuported algorithm"),
        }
        .map_err(Into::into)
    }

    pub fn insert_verification_method_id(
        &mut self,
        key: StorageKey,
        verification_method_id: DIDUrl,
    ) -> anyhow::Result<()> {
        self.verification_method_ids.insert(key, verification_method_id);
        Ok(())
    }

    pub fn get_verification_method_id(&self, key: StorageKey) -> Option<DIDUrl> {
        self.verification_method_ids.get(&key).cloned()
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct StorageKey {
    pub did_method: SupportedDidMethod,
    pub algorithm: Algorithm,
}

impl StorageKey {
    pub fn new(did_method: SupportedDidMethod, algorithm: Algorithm) -> Self {
        Self { did_method, algorithm }
    }
}

pub async fn stronghold_storage() -> StrongholdExtStorage {
    #[cfg(feature = "test_utils")]
    iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    info!("Initializing Stronghold storage");

    let stronghold_password = config().secret_manager.stronghold_password.clone();
    let stronghold_path = config().secret_manager.stronghold_path.clone();

    info!("Stronghold path: {stronghold_path}");

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(stronghold_path)
        .expect("Failed to initialize stronghold adapter");

    let stronghold_storage = StrongholdExtStorage::new(stronghold_adapter);

    info!("Stronghold storage initialized");

    let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();
    let es256_key_id = config().secret_manager.issuer_es256_key_id.clone();

    // Generate keys if they don't exist
    // TODO: currently `generate` will generate a 'static' key-ids for each keytype. In a future improvement we need to
    // make sure that the key-ids are generated dynamically and stored in some sort of key manager.
    if stronghold_storage
        .get_ed25519_public_key(&ed25519_key_id)
        .await
        .is_err()
    {
        info!("Generating new key: {ed25519_key_id}",);
        let key_id = generate(&stronghold_storage, KeyType::new("Ed25519"), JwsAlgorithm::EdDSA)
            .await
            .expect("Failed to generate Ed25519 key");
        assert_eq!(key_id, ed25519_key_id);
    }
    if stronghold_storage.get_es256_public_key(&es256_key_id).await.is_err() {
        info!("Generating new key: {es256_key_id}",);
        let key_id = generate(&stronghold_storage, KeyType::new("ES256"), JwsAlgorithm::ES256)
            .await
            .expect("Failed to generate ES256 key");
        assert_eq!(key_id, es256_key_id);
    }

    stronghold_storage
}

pub async fn generate(
    stronghold_ext_storage: &StrongholdExtStorage,
    key_type: KeyType,
    alg: JwsAlgorithm,
) -> anyhow::Result<KeyId> {
    let key_id = stronghold_ext_storage.generate(key_type.clone(), alg).await?.key_id;

    info!("Generated new {key_type} key with key ID {key_id}");

    Ok(key_id)
}
