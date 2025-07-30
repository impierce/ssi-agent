use agent_shared::config::Logo;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ProfileCommand {
    CreateProfile {
        profile_id: String,
        display_name: Option<String>,
        logo: Option<Logo>,
        provisioned: Option<bool>,
    },
}
