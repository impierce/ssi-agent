use agent_shared::config::Logo;
use serde::Deserialize;

use crate::profile::aggregate::Source;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ProfileCommand {
    CreateProfile {
        profile_id: String,
        display_name: Option<String>,
        logo: Option<Logo>,
        source: Source,
    },
    UpdateDisplayName {
        display_name: Option<String>,
        source: Source,
    },
    UpdateLogo {
        logo: Option<Logo>,
        source: Source,
    },
    UpdateSource {
        source: Source,
    },
}
