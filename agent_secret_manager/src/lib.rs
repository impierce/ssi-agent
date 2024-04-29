use agent_shared::config;
use did_manager::SecretManager;

pub mod aggregate;
pub mod commands;
pub mod events;
pub mod services;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let snapshot_path = config!("stronghold_path");
    let password = config!("stronghold_password");
    let key_id = config!("issuer_key_id");
    let issuer_did = config!("issuer_did");
    let issuer_fragment = config!("issuer_fragment");

    match (snapshot_path, password, key_id, issuer_did, issuer_fragment) {
        (Ok(snapshot_path), Ok(password), Ok(key_id), Ok(issuer_did), Ok(issuer_fragment)) => {
            SecretManager::load(snapshot_path, password, key_id, Some(issuer_did), Some(issuer_fragment)).await.unwrap()
        }
        (Ok(snapshot_path), Ok(password), Ok(key_id), _, _) => {
            SecretManager::load(snapshot_path, password, key_id, None, None).await.unwrap()
        }
        (Ok(snapshot_path), Ok(password), _, _, _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
        _ => panic!("Unable to load or generate `SecretManager`. Please make sure to set both `AGENT_SECRET_MANAGER_STRONGHOLD_PATH` and `AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD` environment variables."),
    }
}
