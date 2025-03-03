use agent_shared::config::{config, SecretManagerConfig};
use did_manager::{InMemoryCache, SecretManager};
use log::info;

pub mod service;
pub mod subject;

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let SecretManagerConfig {
        stronghold_path: snapshot_path,
        stronghold_password: password,
        issuer_eddsa_key_id,
        issuer_es256_key_id,
    } = config().secret_manager.clone();

    info!("{:?}", config().secret_manager);

    let mut builder = SecretManager::builder()
        .snapshot_path(&snapshot_path)
        .password(&password);

    if let Some(issuer_eddsa_key_id) = issuer_eddsa_key_id {
        builder = builder.with_ed25519_key(&issuer_eddsa_key_id);
    }

    if let Some(issuer_es256_key_id) = issuer_es256_key_id {
        builder = builder.with_es256_key(&issuer_es256_key_id);
    }

    if let Some(did_document_cache) = config().did_document_cache.clone() {
        if did_document_cache.enabled {
            let mut cache_builder = InMemoryCache::builder();

            if let Some(ttl) = did_document_cache.ttl {
                cache_builder = cache_builder.ttl(ttl);
            }

            if let Some(include) = did_document_cache.include {
                cache_builder = cache_builder.include(include);
            }

            info!("Enabling DID Document cache with ttl={:?}", did_document_cache.ttl);

            builder = builder.with_cache(cache_builder.build());
        }
    }

    builder.build().await.unwrap()
}
