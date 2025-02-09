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

// TODO: Once we have a proper state implementation for `agent_secret_manager` we can make use of randomly generated Key
// IDs. For now we need to make use of these static variables.
pub static ED25519_KEY_ID: &str = "ed25519-0";
pub static ES256_KEY_ID: &str = "es256-0";

// TODO: the stronghold path does not need to be configured through the config file anymore. Is this static variable for
// the stronghold path the right solution?
pub static STRONGHOLD_PATH: &str = "./app/res/stronghold";

// TODO: find better solution for this
pub async fn stronghold_storage() -> StrongholdExtStorage {
    info!("Initializing Stronghold storage");

    let stronghold_password = config().secret_manager.stronghold_password.clone();

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(STRONGHOLD_PATH)
        .expect("Failed to initialize stronghold adapter");

    let stronghold_storage = StrongholdExtStorage::new(stronghold_adapter);

    info!("Stronghold storage initialized");

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
            .expect("Failed to generate Ed25519 key");
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
            .expect("Failed to generate ES256 key");
        assert_eq!(es256_key_id.as_str(), ES256_KEY_ID);
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
