use agent_shared::config;
use anyhow::Result;
use did_manager::SecretManager;

pub struct SecretManagerServices {
    pub secret_manager: Option<SecretManager>,
    pub default_did_method: String,
}

impl SecretManagerServices {
    pub fn new(secret_manager: Option<SecretManager>) -> Self {
        let default_did_method = config!("default_did_method").unwrap_or("did:key".to_string());
        Self {
            secret_manager,
            default_did_method,
        }
    }

    pub async fn init(&mut self) -> Result<(), std::io::Error> {
        let snapshot_path = config!("stronghold_path").unwrap();
        let password = config!("stronghold_password").unwrap();
        let key_id = config!("issuer_key_id").unwrap();

        let secret_manager = SecretManager::load(snapshot_path, password, key_id).await.unwrap();

        self.secret_manager.replace(secret_manager);

        Ok(())
    }
}
