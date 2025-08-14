use super::aggregate::Profile;
use super::event::ProfileEvent;
use cqrs_es::{EventEnvelope, View};

pub type ProfileView = Profile;

impl View<Profile> for Profile {
    fn update(&mut self, event: &EventEnvelope<Profile>) {
        use ProfileEvent::*;

        match &event.payload {
            ProfileCreated {
                profile_id,
                display_name,
                logo,
                country,
                source,
            } => {
                self.profile_id.clone_from(profile_id);
                self.display_name.clone_from(display_name);
                self.logo.clone_from(logo);
                self.country.clone_from(country);
                self.source.clone_from(source);
            }
            DisplayNameUpdated { display_name, source } => {
                self.display_name.replace(display_name.clone());
                self.source.clone_from(source);
            }
            LogoUpdated { logo, source } => {
                self.logo.clone_from(logo);
                self.source.clone_from(source);
            }
            CountryUpdated { country, source } => {
                self.country.clone_from(country);
                self.source.clone_from(source);
            }
            SourceUpdated { source } => {
                self.source.clone_from(source);
            }
        }
    }
}
