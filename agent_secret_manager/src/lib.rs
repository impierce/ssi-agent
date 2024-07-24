use agent_shared::config::{config, SecretManagerConfig};
use did_manager::SecretManager;

pub mod aggregate;
pub mod commands;
pub mod events;
pub mod services;
pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let SecretManagerConfig {
        stronghold_path: snapshot_path,
        stronghold_password: password,
        issuer_key_id: key_id,
        issuer_did,
        issuer_fragment,
    } = config().secret_manager.clone();

    match (snapshot_path, password, key_id, issuer_did, issuer_fragment) {
        (snapshot_path, password, Some(key_id), issuer_did, issuer_fragment) => {
            SecretManager::load(snapshot_path, password, key_id, issuer_did, issuer_fragment)
                .await
                .unwrap()
        }
        (snapshot_path, password, None, _, _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
    }
}
