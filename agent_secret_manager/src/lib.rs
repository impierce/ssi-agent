use agent_shared::config::{config, get_all_enabled_did_methods, SecretManagerConfig};
use did_manager::{generate, InMemoryCache, SecretManager, StrongholdExtStorage};
use identity_iota::{storage::KeyType, verification::jws::JwsAlgorithm};
use iota_sdk::client::secret::stronghold::StrongholdSecretManager;
use log::info;

pub mod service;
pub mod subject;

pub const STRONGHOLD_PATH: &str = "./agent_secret_manager/strong.hold";
pub const ED25519_KEY_ID: &str = "ed25519-0";
pub const ES256_KEY_ID: &str = "es256-0";
pub const ES256K_KEY_ID: &str = "es256k-0";

// TODO: find better solution for this
pub async fn stronghold_storage() -> StrongholdExtStorage {
    let stronghold_password = config().secret_manager.stronghold_password.clone();

    let stronghold_adapter = StrongholdSecretManager::builder()
        .password(stronghold_password.clone())
        .build(STRONGHOLD_PATH)
        .unwrap();

    let stronghold_storage = StrongholdExtStorage::new(stronghold_adapter);

    // Generate keys
    // TODO: currently `generate` will generate a 'static' key-ids for each keytype. In a future improvement we need to
    // make sure that the key-ids are generated dynamically and stored in some sort of key manager.
    let ed25519_key_id = generate(&stronghold_storage, KeyType::new("Ed25519"), JwsAlgorithm::EdDSA)
        .await
        .unwrap();
    assert_eq!(ed25519_key_id.as_str(), ED25519_KEY_ID);
    let es256_key_id = generate(&stronghold_storage, KeyType::new("ES256"), JwsAlgorithm::ES256)
        .await
        .unwrap();
    assert_eq!(es256_key_id.as_str(), ES256_KEY_ID);
    let es256k_key_id = generate(&stronghold_storage, KeyType::new("ES256K"), JwsAlgorithm::ES256)
        .await
        .unwrap();
    assert_eq!(es256k_key_id.as_str(), ES256K_KEY_ID);

    stronghold_storage
}

// TODO: find better solution for this
pub async fn secret_manager() -> SecretManager {
    let SecretManagerConfig {
        stronghold_password: password,
    } = config().secret_manager.clone();

    info!("{:?}", config().secret_manager);

    let mut builder = SecretManager::builder()
        .snapshot_path(STRONGHOLD_PATH)
        .password(&password);

    // if let Some(issuer_eddsa_key_id) = issuer_eddsa_key_id {
    //     builder = builder.with_ed25519_key(&issuer_eddsa_key_id);
    // }

    // if let Some(issuer_es256_key_id) = issuer_es256_key_id {
    //     builder = builder.with_es256_key(&issuer_es256_key_id);
    // }

    // If `did:iota:rms` is enabled, further values are required.
    // if get_all_enabled_did_methods().contains(&agent_shared::config::SupportedDidMethod::IotaRms) {
    //     builder =
    //         builder
    //             .with_did(
    //                 &issuer_did
    //                     .expect("`You have enabled did:iota:rms, which requires a known DID. Please provide the value through the config or environment variable.`"),
    //             );
    // .with_fragment(&issuer_fragment.expect(
    //     "`You have enabled did:iota:rms, which requires the fragment identifier of the key to be used. Please provide the value through the config or environment variable.`",
    // ));
    // } else {
    //     if let Some(issuer_did) = issuer_did {
    //         builder = builder.with_did(&issuer_did);
    //     }
    // if let Some(issuer_fragment) = issuer_fragment {
    //     builder = builder.with_fragment(&issuer_fragment);
    // }
    // }

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
