use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AuthorizationServerConfigEvent {
    AuthorizationServerConfigCreated {
        authorization_server_config_id: String,
        grant_types_supported: Vec<String>,
        response_types_supported: Vec<String>,
        scopes_supported: Vec<String>,
        authorization_endpoint: String,
        token_endpoint: String,
        jwks_uri: Option<String>,
    },
}

impl DomainEvent for AuthorizationServerConfigEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
