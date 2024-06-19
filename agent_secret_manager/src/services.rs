use crate::subject::Subject;
use agent_shared::config;
use anyhow::Result;
use did_manager::SecretManager;

pub struct SecretManagerServices {
    pub subject: Option<Subject>,
    pub default_did_method: String,
}

impl SecretManagerServices {
    pub fn new(subject: Option<Subject>) -> Self {
        let default_did_method = config!("subject_syntax_types_supported", Vec<String>)
            .ok()
            .and_then(|subject_syntax_types_supported| subject_syntax_types_supported.first().cloned())
            .unwrap_or("did:key".to_string());
        Self {
            subject,
            default_did_method,
        }
    }

    pub async fn init(&mut self) -> Result<(), std::io::Error> {
        let snapshot_path = config!("stronghold_path", String).unwrap();
        let password = config!("stronghold_password", String).unwrap();
        let key_id = config!("issuer_key_id", String).unwrap();
        let issuer_did = config!("issuer_did", String);
        let issuer_fragment = config!("issuer_fragment", String);

        let secret_manager =
            SecretManager::load(snapshot_path, password, key_id, issuer_did.ok(), issuer_fragment.ok())
                .await
                .unwrap();

        self.subject.replace(Subject { secret_manager });

        Ok(())
    }
}
