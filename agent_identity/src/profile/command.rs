use identity_core::common::Url;
use identity_did::DIDUrl;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ProfileCommand {
    CreateProfile {
        profile_id: String,
        display_name: Option<String>,
        logo_uri: Option<Url>,
        provisioned: bool,
    },
}
