use crate::{state::SecretManagerState, stronghold_storage};
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use std::sync::{Arc, OnceLock};

pub struct SecretManagerServices {
    pub stronghold_storage: StrongholdExtStorage,
}

impl SecretManagerServices {
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        Self { stronghold_storage }
    }
}

pub static SECRET_MANAGER_DOMAIN_SERVICE: OnceLock<Arc<SecretManagerDomainService>> = OnceLock::new();

pub struct SecretManagerDomainService {
    pub secret_manager_state: Arc<SecretManagerState>,
}

impl SecretManagerDomainService {
    pub fn new(secret_manager_state: Arc<SecretManagerState>) -> Self {
        Self { secret_manager_state }
    }
}

impl std::fmt::Debug for SecretManagerDomainService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretManagerDomainService").finish()
    }
}
