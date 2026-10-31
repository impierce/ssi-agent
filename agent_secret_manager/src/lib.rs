use agent_shared::{config::config, profile::ApplicationProfile};
use identity_kit::keys::{KeySpec, KeyStore};
use key_store::{issuer_keys, key_type, StrongholdKeyStore};
use log::info;

pub mod key_store;
pub mod service;
pub mod signer;
pub mod subject;

pub async fn stronghold_key_store() -> StrongholdKeyStore {
    #[cfg(feature = "test_utils")]
    iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();

    // TODO: security: this is potentially insecure, as it would allow creating a weakly encrypted Stronghold during development which could be taken to production
    // Can the "work factor" be detected and checked for an existing Stronghold file to prevent its usage in a production profile?
    if let ApplicationProfile::Development = ApplicationProfile::load() {
        iota_stronghold::engine::snapshot::try_set_encrypt_work_factor(0).unwrap();
    }

    info!("Initializing Stronghold storage");

    let stronghold_password = config().secret_manager.stronghold_password.clone();
    #[cfg(feature = "test_utils")]
    let stronghold_path = {
        // Manifest-relative test stronghold path ensures tests find the test keys
        // regardless of the current working directory across workspaces.
        // See `docs/adr/0003-hermetic-test-architecture-and-config-decoupling.md` for context.
        let manifest_relative = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/res/test.stronghold.dat");
        if std::path::Path::new(manifest_relative).exists() {
            manifest_relative.to_string()
        } else {
            config().secret_manager.stronghold_path.clone()
        }
    };

    #[cfg(not(feature = "test_utils"))]
    let stronghold_path = config().secret_manager.stronghold_path.clone();

    info!("Stronghold path: {stronghold_path}");

    let store = StrongholdKeyStore::open(stronghold_path, &stronghold_password, issuer_keys())
        .expect("Failed to open the Stronghold snapshot");

    info!("Stronghold storage initialized");

    // Generate keys if they don't exist
    // TODO: currently `generate` will generate a 'static' key-ids for each keytype. In a future improvement we need to
    // make sure that the key-ids are generated dynamically and stored in some sort of key manager.
    for (key, alg) in issuer_keys() {
        if !store
            .exists(&key)
            .await
            .expect("Failed to read the Stronghold snapshot")
        {
            info!("Generating new key: {key}");
            let key_type = key_type(alg).expect("the issuer keys sign with algorithms the store generates");
            store
                .generate(KeySpec::new(key_type, alg).with_label(key.as_str()))
                .await
                .unwrap_or_else(|error| panic!("Failed to generate {alg} key `{key}`: {error}"));
        }
    }

    store
}
