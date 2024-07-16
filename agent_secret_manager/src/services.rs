use crate::subject::Subject;
use agent_shared::config::{config, did_method_preferred};
use anyhow::Result;
use did_manager::SecretManager;

pub struct SecretManagerServices {
    pub subject: Option<Subject>,
    pub default_did_method: String,
}

impl SecretManagerServices {
    pub fn new(subject: Option<Subject>) -> Self {
        let default_did_method = did_method_preferred();
        Self {
            subject,
            default_did_method,
        }
    }

    pub async fn init(&mut self) -> Result<(), std::io::Error> {
        let snapshot_path = config().secret_manager.stronghold_path;
        let password = config().secret_manager.stronghold_password;
        let key_id = config()
            .secret_manager
            .issuer_key_id
            .expect("Missing configuration: secret_manager.issuer_key_id");
        let issuer_did = config().secret_manager.issuer_did;
        let issuer_fragment = config().secret_manager.issuer_fragment;

        let secret_manager = SecretManager::load(snapshot_path, password, key_id, issuer_did, issuer_fragment)
            .await
            .unwrap();

        self.subject.replace(Subject { secret_manager });

        Ok(())
    }
}
