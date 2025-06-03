pub mod all_authorization_codes;

use super::aggregate::AuthorizationCode;
use cqrs_es::{EventEnvelope, View};

pub type AuthorizationCodeView = AuthorizationCode;

impl View<AuthorizationCode> for AuthorizationCode {
    fn update(&mut self, event: &EventEnvelope<AuthorizationCode>) {
        match &event.payload {
            _ => todo!(),
        }
    }
}
