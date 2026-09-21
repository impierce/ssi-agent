use agent_shared::config::Logo;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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

impl CommandOperation for ProfileCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateProfile { .. } => "identity.profile.create",
            Self::UpdateDisplayName { .. } => "identity.profile.display_name.update",
            Self::UpdateDescription { .. } => "identity.profile.description.update",
            Self::UpdateLogo { .. } => "identity.profile.logo.update",
            Self::UpdateCountry { .. } => "identity.profile.country.update",
            Self::UpdateSource { .. } => "identity.profile.source.update",
        }
    }
}
