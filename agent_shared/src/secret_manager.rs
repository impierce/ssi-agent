use crate::config::config;
use did_manager::SecretManager;

pub async fn secret_manager() -> SecretManager {
    let snapshot_path = config(std::env!("CARGO_PKG_NAME"))
        .get_string("stronghold_path")
        .unwrap();
    let password = config(std::env!("CARGO_PKG_NAME"))
        .get_string("stronghold_password")
        .unwrap();
    let key_id = config(std::env!("CARGO_PKG_NAME")).get_string("issuer_key_id").unwrap();

    SecretManager::load(snapshot_path, password, key_id).await.unwrap()
}
