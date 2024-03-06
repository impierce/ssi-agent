use agent_shared::config;
use identity_stronghold::StrongholdStorage;
use iota_sdk::client::{secret::stronghold::StrongholdSecretManager, Password};
use iota_stronghold::SnapshotPath;
use log::info;

pub struct SecretManager {
    pub stronghold_storage: StrongholdStorage,
}

impl SecretManager {
    /// Generates a new Stronghold
    pub fn generate() -> Self {
        // TODO: require specifying the path and password?

        info!("Creating new Stronghold ...");
        let stronghold_secret_manager = StrongholdSecretManager::builder()
            .password(Password::from("test123".to_string()))
            .build("test123")
            .unwrap();
        SecretManager {
            stronghold_storage: StrongholdStorage::new(stronghold_secret_manager),
        }
    }

    /// Loads an existing Stronghold as specified in the environment variables
    pub fn load() -> Self {
        let snapshot_path_str = config!("stronghold_path").unwrap();
        let snapshot_path = SnapshotPath::from_path(snapshot_path_str);

        info!("Loading existing Stronghold from {:?} ...", snapshot_path.as_path());

        let password = config!("stronghold_password").unwrap();

        let stronghold_secret_manager = StrongholdSecretManager::builder()
            .password(Password::from(password))
            .build(snapshot_path.as_path())
            .unwrap();

        SecretManager {
            stronghold_storage: StrongholdStorage::new(stronghold_secret_manager),
        }
    }
}
