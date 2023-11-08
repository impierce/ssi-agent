use anyhow::Result;
use identity_credential::credential::Credential;

use crate::KeyManager;

/// Connector to [HashiCorp Vault](https://www.vaultproject.io/) for key management
pub struct VaultKeyManager {}

impl KeyManager for VaultKeyManager {
    fn sign(credential: Credential) -> Result<Credential> {
        todo!()
    }

    fn create_verification_method() -> Result<String> {
        todo!()
    }
}

// use identity.rs storage interface: in-mem (stronghold), all other kms via
// https://github.com/iotaledger/identity.rs/tree/main/identity_storage/src
// jwk
