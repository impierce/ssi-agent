use agent_shared::config::{config, SecretManagerConfig};
use did_manager::SecretManager;
use log::info;

pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let SecretManagerConfig {
        generate_stronghold,
        stronghold_path: snapshot_path,
        stronghold_password: password,
        issuer_eddsa_key_id,
        issuer_es256_key_id,
        issuer_did,
        issuer_fragment,
    } = config().secret_manager.clone();

    if generate_stronghold {
        info!("Generating new secret manager");
        SecretManager::generate(snapshot_path, password).await.unwrap()
    } else {
        info!("Loading secret manager from Stronghold snapshot");
        SecretManager::load(
            snapshot_path,
            password,
            issuer_eddsa_key_id,
            issuer_es256_key_id,
            None,
            issuer_did,
            issuer_fragment,
        )
        .await
        .unwrap()
    }
}
