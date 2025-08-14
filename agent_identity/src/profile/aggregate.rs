use agent_shared::config::Logo;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use crate::services::IdentityServices;

use super::{command::ProfileCommand, error::ProfileError, event::ProfileEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum Source {
    Provisioned,
    Default,
    Runtime,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    #[serde(rename = "id")]
    pub profile_id: String,
    pub display_name: Option<String>,
    pub logo: Option<Logo>,
    pub country: Option<String>,
    pub source: Source,
}

#[async_trait]
impl Aggregate for Profile {
    type Command = ProfileCommand;
    type Event = ProfileEvent;
    type Error = ProfileError;
    type Services = Arc<IdentityServices>;

    fn aggregate_type() -> String {
        "profile".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use ProfileCommand::*;
        use ProfileEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateProfile {
                profile_id,
                display_name,
                logo,
                country,
                source,
            } => {
                debug!("Creating profile with ID: {}", profile_id);

                if source == Source::Runtime && self.source == Source::Provisioned {
                    return Err(ProfileError::ConfigurationConflict);
                }

                Ok(vec![ProfileCreated {
                    profile_id,
                    display_name,
                    logo,
                    country,
                    source,
                }])
            }
            UpdateDisplayName { display_name, source } => {
                debug!("Updating display name: {:?}", display_name);

                if source == Source::Runtime && self.source == Source::Provisioned {
                    return Err(ProfileError::ConfigurationConflict);
                }

                Ok(vec![ProfileEvent::DisplayNameUpdated { display_name, source }])
            }
            UpdateLogo { logo, source } => {
                debug!("Updating logo: {:?}", logo);

                if source == Source::Runtime && self.source == Source::Provisioned {
                    return Err(ProfileError::ConfigurationConflict);
                }

                Ok(vec![ProfileEvent::LogoUpdated { logo, source }])
            }
            UpdateCountry { country, source } => {
                debug!("Updating country: {:?}", country);

                if source == Source::Runtime && self.source == Source::Provisioned {
                    return Err(ProfileError::ConfigurationConflict);
                }

                Ok(vec![ProfileEvent::CountryUpdated { country, source }])
            }
            UpdateSource { source } => {
                debug!("Updating source: {:?}", source);

                Ok(vec![ProfileEvent::SourceUpdated { source }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use ProfileEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ProfileCreated {
                profile_id,
                display_name,
                logo,
                country,
                source,
            } => {
                self.profile_id = profile_id;
                self.display_name = display_name;
                self.logo = logo;
                self.country = country;
                self.source = source;
            }
            DisplayNameUpdated { display_name, source } => {
                self.display_name.replace(display_name);
                self.source = source;
            }
            LogoUpdated { logo, source } => {
                self.logo = logo;
                self.source = source;
            }
            CountryUpdated { country, source } => {
                self.country = country;
                self.source = source;
            }
            SourceUpdated { source } => {
                self.source = source;
            }
        }
    }
}
