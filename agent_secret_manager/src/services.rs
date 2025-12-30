use crate::stronghold_storage;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;

pub struct SecretManagerServices {
    pub stronghold_storage: StrongholdExtStorage,
}

impl SecretManagerServices {
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        Self { stronghold_storage }
    }
}
