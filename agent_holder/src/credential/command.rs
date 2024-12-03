use identity_credential::credential::Jwt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    AddCredential {
        holder_credential_id: String,
        received_offer_id: String,
        credential: Jwt,
    },
}
