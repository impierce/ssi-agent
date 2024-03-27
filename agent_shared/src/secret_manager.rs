use crate::config::config;
use did_manager::SecretManager;

pub async fn secret_manager() -> SecretManager {
    let snapshot_path = config(std::env!("CARGO_PKG_NAME")).get_string("stronghold_path");
    let password = config(std::env!("CARGO_PKG_NAME")).get_string("stronghold_password");
    let key_id = config(std::env!("CARGO_PKG_NAME")).get_string("issuer_key_id");

    match (snapshot_path, password, key_id) {
        (Ok(snapshot_path), Ok(password), Ok(key_id)) => {
            SecretManager::load(snapshot_path, password, key_id).await.unwrap()
        }
        (Ok(snapshot_path), Ok(password), _) => SecretManager::generate(snapshot_path, password).await.unwrap(),
        _ => panic!("Unable to load or generate `SecretManager`. Please make sure to set both `AGENT_CONFIG_STRONGHOLD_PATH` and `AGENT_CONFIG_STRONGHOLD_PASSWORD` environment variables."),
    }
}
