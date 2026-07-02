use agent_shared::config::Logo;
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use crate::services::IdentityServices;

use super::{command::ProfileCommand, error::ProfileError, event::ProfileEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
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
    pub description: Option<String>,
    pub logo: Option<Logo>,
    pub country: Option<String>,
    pub source: Source,
}

impl Aggregate for Profile {
    type Command = ProfileCommand;
    type Event = ProfileEvent;
    type Error = ProfileError;
    type Services = Arc<IdentityServices>;

    const TYPE: &'static str = "profile";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use ProfileCommand::*;
        use ProfileEvent::*;

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            CreateProfile {
                profile_id,
                display_name,
                description,
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
                    description,
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
            UpdateDescription { description, source } => {
                debug!("Updating description: {:?}", description);

                if source == Source::Runtime && self.source == Source::Provisioned {
                    return Err(ProfileError::ConfigurationConflict);
                }

                Ok(vec![ProfileEvent::DescriptionUpdated { description, source }])
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
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use ProfileEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            ProfileCreated {
                profile_id,
                display_name,
                description,
                logo,
                country,
                source,
            } => {
                self.profile_id = profile_id;
                self.display_name = display_name;
                self.description = description;
                self.logo = logo;
                self.country = country;
                self.source = source;
            }
            DisplayNameUpdated { display_name, source } => {
                self.display_name.replace(display_name);
                self.source = source;
            }
            DescriptionUpdated { description, source } => {
                self.description = description;
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
