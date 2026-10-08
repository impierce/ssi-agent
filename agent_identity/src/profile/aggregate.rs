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

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::test::TestFramework;

    type ProfileTestFramework = TestFramework<Profile>;

    fn services() -> Arc<IdentityServices> {
        IdentityServices::default()
    }

    fn profile_created(source: Source) -> ProfileEvent {
        ProfileEvent::ProfileCreated {
            profile_id: "profile-id".to_string(),
            display_name: Some("Example Organisation".to_string()),
            description: None,
            logo: None,
            country: None,
            source,
        }
    }

    fn runtime_updates() -> Vec<ProfileCommand> {
        vec![
            ProfileCommand::CreateProfile {
                profile_id: "profile-id".to_string(),
                display_name: Some("Other Organisation".to_string()),
                description: None,
                logo: None,
                country: None,
                source: Source::Runtime,
            },
            ProfileCommand::UpdateDisplayName {
                display_name: "Other Organisation".to_string(),
                source: Source::Runtime,
            },
            ProfileCommand::UpdateDescription {
                description: Some("Description".to_string()),
                source: Source::Runtime,
            },
            ProfileCommand::UpdateLogo {
                logo: Some(Logo {
                    uri: Some("https://example.com/logo.png".parse().unwrap()),
                    alt_text: None,
                }),
                source: Source::Runtime,
            },
            ProfileCommand::UpdateCountry {
                country: Some("NL".to_string()),
                source: Source::Runtime,
            },
        ]
    }

    #[test]
    fn create_profile() {
        ProfileTestFramework::with(services())
            .given_no_previous_events()
            .when(ProfileCommand::CreateProfile {
                profile_id: "profile-id".to_string(),
                display_name: Some("Example Organisation".to_string()),
                description: None,
                logo: None,
                country: None,
                source: Source::Default,
            })
            .then_expect_events(vec![profile_created(Source::Default)]);
    }

    #[test]
    fn runtime_changes_are_rejected_for_a_provisioned_profile() {
        for command in runtime_updates() {
            ProfileTestFramework::with(services())
                .given(vec![profile_created(Source::Provisioned)])
                .when(command)
                .then_expect_error_message(&ProfileError::ConfigurationConflict.to_string());
        }
    }

    #[test]
    fn runtime_changes_are_accepted_for_a_default_profile() {
        for command in runtime_updates() {
            let result = ProfileTestFramework::with(services())
                .given(vec![profile_created(Source::Default)])
                .when(command)
                .inspect_result();

            assert!(result.is_ok(), "unexpected error: {result:?}");
        }
    }

    #[test]
    fn provisioned_changes_are_accepted_for_a_provisioned_profile() {
        ProfileTestFramework::with(services())
            .given(vec![profile_created(Source::Provisioned)])
            .when(ProfileCommand::UpdateDisplayName {
                display_name: "Other Organisation".to_string(),
                source: Source::Provisioned,
            })
            .then_expect_events(vec![ProfileEvent::DisplayNameUpdated {
                display_name: "Other Organisation".to_string(),
                source: Source::Provisioned,
            }]);
    }

    #[test]
    fn source_can_be_updated_for_a_provisioned_profile() {
        ProfileTestFramework::with(services())
            .given(vec![profile_created(Source::Provisioned)])
            .when(ProfileCommand::UpdateSource {
                source: Source::Default,
            })
            .then_expect_events(vec![ProfileEvent::SourceUpdated {
                source: Source::Default,
            }]);
    }

    #[test]
    fn applying_events_matches_the_view() {
        use cqrs_es::{EventEnvelope, View as _};
        use std::collections::HashMap;

        let logo = Logo {
            uri: Some("https://example.com/logo.png".parse().unwrap()),
            alt_text: Some("Logo".to_string()),
        };
        let events = vec![
            profile_created(Source::Default),
            ProfileEvent::DisplayNameUpdated {
                display_name: "Other Organisation".to_string(),
                source: Source::Runtime,
            },
            ProfileEvent::DescriptionUpdated {
                description: Some("Description".to_string()),
                source: Source::Runtime,
            },
            ProfileEvent::LogoUpdated {
                logo: Some(logo.clone()),
                source: Source::Runtime,
            },
            ProfileEvent::CountryUpdated {
                country: Some("NL".to_string()),
                source: Source::Runtime,
            },
            ProfileEvent::SourceUpdated {
                source: Source::Provisioned,
            },
        ];

        let mut aggregate = Profile::default();
        let mut view = crate::profile::views::ProfileView::default();
        for (sequence, event) in events.into_iter().enumerate() {
            view.update(&EventEnvelope {
                aggregate_id: "profile-id".to_string(),
                sequence: sequence + 1,
                payload: event.clone(),
                metadata: HashMap::new(),
            });
            aggregate.apply(event);
        }

        for profile in [aggregate, view] {
            assert_eq!(profile.profile_id, "profile-id");
            assert_eq!(profile.display_name.as_deref(), Some("Other Organisation"));
            assert_eq!(profile.description.as_deref(), Some("Description"));
            assert_eq!(profile.logo, Some(logo.clone()));
            assert_eq!(profile.country.as_deref(), Some("NL"));
            assert_eq!(profile.source, Source::Provisioned);
        }
    }
}
