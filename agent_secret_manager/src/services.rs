use crate::subject::Subject;
use agent_shared::config::{config, get_preferred_did_method, SecretManagerConfig, SupportedDidMethod};
use anyhow::Result;
use did_manager::SecretManager;

pub struct SecretManagerServices {
    pub subject: Option<Subject>,
    pub default_did_method: SupportedDidMethod,
}

impl SecretManagerServices {
    pub fn new(subject: Option<Subject>) -> Self {
        let default_did_method = get_preferred_did_method();
        Self {
            subject,
            default_did_method,
        }
    }

    pub async fn init(&mut self) -> Result<(), std::io::Error> {
        let SecretManagerConfig {
            stronghold_path: snapshot_path,
            stronghold_password: password,
            issuer_key_id,
            issuer_did,
            issuer_fragment,
        } = config().secret_manager.clone();

        let key_id = issuer_key_id.expect("Missing configuration: secret_manager.issuer_key_id");

        let secret_manager = SecretManager::builder()
            .snapshot_path(&snapshot_path)
            .password(&password)
            .with_ed25519_key(&key_id)
            .with_did(&issuer_did.expect("Missing configuration: secret_manager.issuer_did"))
            .with_fragment(&issuer_fragment.expect("Missing configuration: secret_manager.issuer_fragment"))
            .build()
            .await
            .unwrap();

        self.subject.replace(Subject { secret_manager });

        Ok(())
    }
}
