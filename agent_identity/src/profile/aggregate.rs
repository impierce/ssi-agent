use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_core::common::Url;
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
    pub logo_uri: Option<Url>,
    pub provisioned: bool,
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
                logo_uri,
                provisioned,
            } => {
                debug!("Creating profile with ID: {}", profile_id);

                if !provisioned && self.provisioned {
                    return Err(ProfileError::AlreadyProvisioned);
                }

                Ok(vec![ProfileEvent::ProfileCreated {
                    profile_id,
                    display_name,
                    logo_uri,
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
                logo_uri,
                provisioned,
            } => {
                self.profile_id = profile_id;
                self.display_name = display_name;
                self.logo_uri = logo_uri;
                self.provisioned = provisioned;
            }
        }
    }
}
