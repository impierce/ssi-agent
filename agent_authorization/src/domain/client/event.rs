use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum ClientEvent {
    ClientRegistered {
        client_id: String,
        client_secret: Option<String>,
        client_name: Option<String>,
        logo_uri: Option<String>,
        policy_uri: Option<String>,
        tos_uri: Option<String>,
        redirect_uris: Vec<Url>,
        grant_types: Vec<String>,
        response_types: Vec<String>,
        token_endpoint_auth_method: String,
        require_pkce: bool,
        code_challenge_methods_supported: Vec<String>,
        require_pushed_authorization_request: bool,
    },
}

impl DomainEvent for ClientEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
