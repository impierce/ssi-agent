use super::OAuth2AuthorizationRequest;
use super::OAuth2AuthorizationRequestView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllOAuth2AuthorizationRequestsView {
    #[serde(flatten)]
    pub oauth2_authorization_reqests: HashMap<String, OAuth2AuthorizationRequestView>,
}

impl View<OAuth2AuthorizationRequest> for AllOAuth2AuthorizationRequestsView {
    fn update(&mut self, event: &EventEnvelope<OAuth2AuthorizationRequest>) {
        self.oauth2_authorization_reqests
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
