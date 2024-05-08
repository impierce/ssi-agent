use agent_shared::config;
use did_manager::SecretManager;

pub mod aggregate;
pub mod commands;
pub mod events;
pub mod services;
pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let snapshot_path = config!("stronghold_path");
    let password = config!("stronghold_password");
    let key_id = config!("issuer_key_id");

    match (snapshot_path, password, key_id) {
        (Ok(snapshot_path), Ok(password), Ok(key_id)) => {
            SecretManager::load(snapshot_path, password, key_id, None, None).await.unwrap()
        }
        (Ok(snapshot_path), Ok(password), _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
        _ => panic!("Unable to load or generate `SecretManager`. Please make sure to set both `AGENT_SECRET_MANAGER_STRONGHOLD_PATH` and `AGENT_SECRET_MANAGER_STRONGHOLD_PASSWORD` environment variables."),
    }
}
