use agent_shared::config::config;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::{
    storage::{JwkStorage, KeyId, KeyType},
    verification::jws::JwsAlgorithm,
};
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use log::info;

pub mod service;
pub mod subject;

pub const STRONGHOLD_PATH: &str = "./app/res/stronghold";
pub const ED25519_KEY_ID: &str = "ed25519-0";
pub const ES256_KEY_ID: &str = "es256-0";

// TODO: find better solution for this
pub async fn stronghold_storage() -> StrongholdExtStorage {
    // iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    info!("Initializing Stronghold storage");

    let stronghold_password = config().secret_manager.stronghold_password.clone();

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(STRONGHOLD_PATH)
        .unwrap();

    info!(
        "Stronghold storage initialized with password: {:?}",
        stronghold_password
    );

    let stronghold_storage = StrongholdExtStorage::new(stronghold_adapter);

    // Generate keys if they don't exist
    // TODO: currently `generate` will generate a 'static' key-ids for each keytype. In a future improvement we need to
    // make sure that the key-ids are generated dynamically and stored in some sort of key manager.
    if stronghold_storage
        .get_ed25519_public_key(&identity_iota::storage::KeyId::new(ED25519_KEY_ID))
        .await
        .is_err()
    {
        info!(
            "Generating new key: {}",
            identity_iota::storage::KeyId::new(ED25519_KEY_ID)
        );
        let ed25519_key_id = generate(&stronghold_storage, KeyType::new("Ed25519"), JwsAlgorithm::EdDSA)
            .await
            .unwrap();
        assert_eq!(ed25519_key_id.as_str(), ED25519_KEY_ID);
    }
    if stronghold_storage
        .get_es256_public_key(&identity_iota::storage::KeyId::new(ES256_KEY_ID))
        .await
        .is_err()
    {
        info!(
            "Generating new key: {}",
            identity_iota::storage::KeyId::new(ES256_KEY_ID)
        );
        let es256_key_id = generate(&stronghold_storage, KeyType::new("ES256"), JwsAlgorithm::ES256)
            .await
            .unwrap();
        assert_eq!(es256_key_id.as_str(), ES256_KEY_ID);
    }

    info!(
        "Stronghold storage initialized with password: {:?}",
        stronghold_password
    );
    stronghold_storage
}

pub async fn generate(
    stronghold_ext_storage: &StrongholdExtStorage,
    key_type: KeyType,
    alg: JwsAlgorithm,
) -> Result<KeyId, ()> {
    let jwk_gen_output = stronghold_ext_storage
        .generate(key_type.clone(), alg)
        .await
        // FIX THIS
        .map_err(|_| ())?;
    info!(
        "Generated new {:?} key with key ID {:?}",
        &key_type.as_str(),
        &jwk_gen_output.key_id.as_str()
    );
    Ok(jwk_gen_output.key_id)
}
