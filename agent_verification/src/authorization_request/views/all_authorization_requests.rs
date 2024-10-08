use super::AuthorizationRequestView;
use crate::authorization_request::views::AuthorizationRequest;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllAuthorizationRequestsView {
    #[serde(flatten)]
    pub authorization_requests: HashMap<String, AuthorizationRequestView>,
}

impl View<AuthorizationRequest> for AllAuthorizationRequestsView {
    fn update(&mut self, event: &EventEnvelope<AuthorizationRequest>) {
        self.authorization_requests
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
