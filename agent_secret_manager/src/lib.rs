use agent_shared::config::{config, SecretManagerConfig};
use did_manager::{InMemoryCache, SecretManager};

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

    if let Some(key_id) = key_id {
        SecretManager::builder()
            .snapshot_path(&snapshot_path)
            .password(&password)
            .with_ed25519_key(&key_id)
            .with_did(&issuer_did.expect("`issuer_did` missing"))
            .with_fragment(&issuer_fragment.expect("`issuer_fragment` missing"))
            .with_cache(InMemoryCache::builder().ttl(60_000).build())
            .build()
            .await
            .unwrap()
    } else {
        SecretManager::builder()
            .snapshot_path(&snapshot_path)
            .password(&password)
            .build()
            .await
            .unwrap()
    }
}
