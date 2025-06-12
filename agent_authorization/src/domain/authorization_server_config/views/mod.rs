pub mod all_authorization_codes;

use super::aggregate::AuthorizationServerConfig;
use cqrs_es::{EventEnvelope, View};

pub type AuthorizationServerConfigView = AuthorizationServerConfig;

impl View<AuthorizationServerConfig> for AuthorizationServerConfig {
    fn update(&mut self, event: &EventEnvelope<AuthorizationServerConfig>) {
        use super::event::AuthorizationServerConfigEvent::*;

        match &event.payload {
            AuthorizationServerConfigCreated {
                authorization_server_config_id,
                grant_types_supported,
                response_types_supported,
                scopes_supported,
                authorization_endpoint,
                token_endpoint,
                jwks_uri,
            } => {
                self.authorization_server_config_id
                    .clone_from(authorization_server_config_id);
                self.grant_types_supported.clone_from(grant_types_supported);
                self.response_types_supported.clone_from(response_types_supported);
                self.scopes_supported.clone_from(scopes_supported);
                self.authorization_endpoint.clone_from(authorization_endpoint);
                self.token_endpoint.clone_from(token_endpoint);
                self.jwks_uri.clone_from(jwks_uri);
            }
        }
    }
}
