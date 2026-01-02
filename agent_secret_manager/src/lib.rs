use agent_shared::{config::config, profile::ApplicationProfile};
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::{
    storage::{JwkStorage, KeyId, KeyType},
    verification::jws::JwsAlgorithm,
};
use iota_sdk_legacy::client::secret::stronghold::StrongholdSecretManager;
use log::info;

// Aggregates
pub mod managed_key;

pub mod services;
pub mod state;
pub mod subject;

pub async fn stronghold_storage() -> StrongholdExtStorage {
    #[cfg(feature = "test_utils")]
    iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    // TODO: security: this is potentially insecure, as it would allow creating a weakly encrypted Stronghold during development which could be taken to production
    // Can the "work factor" be detected and checked for an existing Stronghold file to prevent its usage in a production profile?
    if let ApplicationProfile::Development = ApplicationProfile::load() {
        iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();
    }

    info!("Initializing Stronghold storage");

    let stronghold_password = config().secret_manager.stronghold_password.clone();
    let stronghold_path = config().secret_manager.stronghold_path.clone();

    info!("Stronghold path: {stronghold_path}");

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(stronghold_path)
        .expect("Failed to initialize stronghold adapter");

    let stronghold_storage = StrongholdExtStorage::new(stronghold_adapter);

    let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();

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

    info!("Stronghold storage initialized");

    stronghold_storage
}

pub async fn generate(
    stronghold_ext_storage: &StrongholdExtStorage,
    key_type: KeyType,
    alg: JwsAlgorithm,
) -> anyhow::Result<KeyId> {
    let key_id = stronghold_ext_storage.generate(key_type.clone(), alg).await?.key_id;

    info!("Generated new `{key_type}` key with key ID `{key_id}`");

    Ok(key_id)
}
