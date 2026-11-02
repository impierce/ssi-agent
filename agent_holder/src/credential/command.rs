use super::signed::SignedCredential;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CredentialCommand {
    AddCredential {
        holder_credential_id: String,
        received_offer_id: Option<String>,
        credential: SignedCredential,
    },
}

impl CommandOperation for CredentialCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::AddCredential { .. } => "holder.credentials.add",
        }
    }
}
