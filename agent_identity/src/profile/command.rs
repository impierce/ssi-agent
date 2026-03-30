use agent_shared::config::Logo;
use serde::Deserialize;

use crate::profile::aggregate::Source;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ProfileCommand {
    CreateProfile {
        profile_id: String,
        display_name: Option<String>,
        description: Option<String>,
        logo: Option<Logo>,
        country: Option<String>,
        source: Source,
    },
    UpdateDisplayName {
        display_name: String,
        source: Source,
    },
    UpdateDescription {
        description: Option<String>,
        source: Source,
    },
    UpdateLogo {
        logo: Option<Logo>,
        source: Source,
    },
    UpdateCountry {
        country: Option<String>,
        source: Source,
    },
    UpdateSource {
        source: Source,
    },
}
