use agent_shared::config::config_2;
use did_manager::SecretManager;

pub mod aggregate;
pub mod commands;
pub mod events;
pub mod services;
pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let snapshot_path = config_2().secret_manager.stronghold_path;
    let password = config_2().secret_manager.stronghold_password;
    let key_id = config_2().secret_manager.issuer_key_id;
    let issuer_did = config_2().secret_manager.issuer_did;
    let issuer_fragment = config_2().secret_manager.issuer_fragment;

    match (snapshot_path, password, key_id, issuer_did, issuer_fragment) {
        (snapshot_path, password, Some(key_id), issuer_did, issuer_fragment) => {
            SecretManager::load(snapshot_path, password, key_id, issuer_did, issuer_fragment).await.unwrap()
        }
        (snapshot_path, password, None, _, _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
        _ => panic!("Unable to load or generate `SecretManager`. Please make sure to set both `AGENT__SECRET_MANAGER__STRONGHOLD_PATH` and `AGENT__SECRET_MANAGER__STRONGHOLD_PASSWORD` environment variables."),
    }
}
