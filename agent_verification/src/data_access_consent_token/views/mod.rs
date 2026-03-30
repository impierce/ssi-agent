pub mod all_data_access_consent_tokens;
use cqrs_es::{EventEnvelope, View};

use crate::data_access_consent_token::aggregate::DataAccessConsentToken;

pub type DataAccessConsentTokenView = DataAccessConsentToken;

impl View<DataAccessConsentToken> for DataAccessConsentToken {
    fn update(&mut self, event: &EventEnvelope<DataAccessConsentToken>) {
        use crate::data_access_consent_token::event::DataAccessConsentTokenEvent::*;

        match &event.payload {
            DataAccessConsentTokenStored { id, token } => {
                self.id = id.clone();
                self.token = token.clone();
            }
            DataAccessConsentTokenResolved { .. } => {
                // This event is only for the purpose of storing the event, no new or updated data needs to be stored.
            }
        }
    }
}
