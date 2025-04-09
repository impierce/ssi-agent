use agent_shared::{config::config, profile::ApplicationProfile};
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::{
    storage::{JwkStorage, KeyId, KeyType},
    verification::jws::JwsAlgorithm,
};
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use log::info;

pub mod service;
pub mod subject;

pub async fn stronghold_storage() -> StrongholdExtStorage {
    #[cfg(feature = "test_utils")]
    iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    match ApplicationProfile::load() {
        ApplicationProfile::Development => {
            iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();
        }
        _ => {}
    }

    info!("Initializing Stronghold storage");

    let stronghold_password = config()
        .secret_manager
        .stronghold_password
        .clone()
        .expect("Stronghold password not set");
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

    info!("Generated new `{key_type}` key with key ID `{key_id}`");

    Ok(key_id)
}
