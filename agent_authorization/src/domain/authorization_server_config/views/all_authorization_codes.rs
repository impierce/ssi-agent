use super::AuthorizationServerConfig;
use super::AuthorizationServerConfigView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllAuthorizationServerConfigsView {
    #[serde(flatten)]
    pub authorization_server_configs: HashMap<String, AuthorizationServerConfigView>,
}

impl View<AuthorizationServerConfig> for AllAuthorizationServerConfigsView {
    fn update(&mut self, event: &EventEnvelope<AuthorizationServerConfig>) {
        self.authorization_server_configs
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
