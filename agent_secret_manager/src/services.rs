use agent_shared::config;
use anyhow::Result;
use oid4vc_core::authentication::sign::ExternalSign;
use producer::SecretManager;

pub struct SecretManagerServices {
    pub secret_manager: Option<SecretManager>,
}

// impl ExternalSign for SecretManagerServices {
//     fn sign(&self, message: &str) -> Result<Vec<u8>> {
//         let signature = tokio::runtime::Runtime::new()
//             .unwrap()
//             .block_on(self.secret_manager.as_ref().unwrap().sign(message.as_bytes()))
//             .unwrap();
//         Ok(signature)
//     }
// }

impl SecretManagerServices {
    pub fn new(secret_manager: Option<SecretManager>) -> Self {
        Self { secret_manager }
    }

    pub fn init(&mut self) -> Result<(), std::io::Error> {
        let snapshot_path = config!("stronghold_path").unwrap();
        let password = config!("stronghold_password").unwrap();

        let secret_manager = SecretManager::load(snapshot_path, password).unwrap();

        self.secret_manager = Some(secret_manager);

        Ok(())
    }
}
