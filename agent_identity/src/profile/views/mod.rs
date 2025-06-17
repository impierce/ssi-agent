use super::aggregate::Profile;
use super::event::ProfileEvent;
use cqrs_es::{EventEnvelope, View};

pub type ProfileView = Profile;

impl View<Profile> for Profile {
    fn update(&mut self, event: &EventEnvelope<Profile>) {
        use ProfileEvent::*;

        match &event.payload {
            _ => todo!(),
        }
    }
}
