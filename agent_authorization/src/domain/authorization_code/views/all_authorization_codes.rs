use super::AuthorizationCode;
use super::AuthorizationCodeView;
use cqrs_es::{EventEnvelope, View};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllAuthorizationCodesView {
    #[serde(flatten)]
    pub authorization_codes: IndexMap<String, AuthorizationCodeView>,
}

impl View<AuthorizationCode> for AllAuthorizationCodesView {
    fn update(&mut self, event: &EventEnvelope<AuthorizationCode>) {
        self.authorization_codes
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
