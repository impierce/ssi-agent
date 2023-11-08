pub mod kms;

use anyhow::Result;
use identity_credential::credential::Credential;

pub trait KeyManager {
    fn sign(credential: Credential) -> Result<Credential>;

    fn create_verification_method() -> Result<String>;
}
