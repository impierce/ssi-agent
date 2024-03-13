use agent_shared::config;
use anyhow::Result;
use did_manager::SecretManager;

pub struct SecretManagerServices {
    pub secret_manager: Option<SecretManager>,
}

impl SecretManagerServices {
    pub fn new(secret_manager: Option<SecretManager>) -> Self {
        Self { secret_manager }
    }

    pub async fn init(&mut self) -> Result<(), std::io::Error> {
        let snapshot_path = config!("stronghold_path").unwrap();
        let password = config!("stronghold_password").unwrap();
        let key_id = config!("issuer_key_id").unwrap();

        let secret_manager = SecretManager::load(snapshot_path, password, key_id).await.unwrap();

        self.secret_manager = Some(secret_manager);

        Ok(())
    }
}
