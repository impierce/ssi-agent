use identity_credential::credential::Jwt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    AddCredential {
        credential_id: String,
        offer_id: String,
        credential: Jwt,
    },
}
