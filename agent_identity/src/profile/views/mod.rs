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
                provisioned,
            } => {
                self.profile_id = profile_id.clone();
                self.display_name = display_name.clone();
                self.logo = logo.clone();
                self.provisioned = *provisioned;
            }
        }
    }
}
