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

    info!("Stronghold storage initialized");

    stronghold_storage
}
