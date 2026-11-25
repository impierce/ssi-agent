use crate::credential::signed::SignedCredential;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PresentationCommand {
    CreatePresentation {
        presentation_id: String,
        signed_credentials: Vec<SignedCredential>,
    },
}

impl CommandOperation for PresentationCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreatePresentation { .. } => "holder.presentations.create",
        }
    }
}
