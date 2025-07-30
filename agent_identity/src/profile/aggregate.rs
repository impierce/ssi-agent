use agent_shared::config::{config, config_mut, Logo};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

use crate::services::IdentityServices;

use super::{command::ProfileCommand, error::ProfileError, event::ProfileEvent};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    #[serde(rename = "id")]
    pub profile_id: String,
    pub display_name: Option<String>,
    pub logo: Option<Logo>,
    pub provisioned: Option<bool>,
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
                provisioned,
            } => {
                debug!("Creating profile with ID: {}", profile_id);

                if provisioned.is_none() && self.provisioned == Some(true) {
                    return Err(ProfileError::AlreadyProvisioned);
                }

                let mut display = config().display.first().cloned().unwrap_or_default();

                if let Some(display_name) = display_name.clone() {
                    display.name = display_name;
                }

                if let Some(logo) = logo.clone() {
                    display.logo = Some(logo.clone());
                }

                config_mut().display = vec![display];

                Ok(vec![ProfileCreated {
                    profile_id,
                    display_name,
                    logo,
                    provisioned,
                }])
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
                provisioned,
            } => {
                self.profile_id = profile_id;
                self.display_name = display_name;
                self.logo = logo;
                self.provisioned = provisioned;
            }
        }
    }
}
