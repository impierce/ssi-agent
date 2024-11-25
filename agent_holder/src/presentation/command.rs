use identity_credential::credential::Jwt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PresentationCommand {
    CreatePresentation {
        presentation_id: String,
        signed_credentials: Vec<Jwt>,
    },
}
