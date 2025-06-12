use super::command::AuthorizationServerConfigCommand;
use super::error::AuthorizationServerConfigError;
use super::event::AuthorizationServerConfigEvent;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct AuthorizationServerConfig {
    #[serde(rename = "id")]
    pub authorization_server_config_id: String,
    pub grant_types_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
    // token_settings?: TokenSettings
    // access_token_lifetime: std::time::Duration
    // refresh_token_lifetime: std::time::Duration
    // id_token_lifetime: std::time::Duration
    // authorization_code_lifetime: std::time::Duration

    // endpoint_paths?: EndpointPaths
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: Option<String>,
    // pub issuer: String,
    // pub registration_endpoint: Option<String>,
    // pub response_modes_supported: Vec<String>,
    // pub token_endpoint_auth_methods_supported: Vec<String>,
    // pub token_endpoint_auth_signing_alg_values_supported: Vec<String>,
    // pub service_documentation: Option<String>,
}

#[async_trait]
impl Aggregate for AuthorizationServerConfig {
    type Command = AuthorizationServerConfigCommand;
    type Event = AuthorizationServerConfigEvent;
    type Error = AuthorizationServerConfigError;
    type Services = ();

    fn aggregate_type() -> String {
        "authorization_server_config".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use AuthorizationServerConfigCommand::*;
        use AuthorizationServerConfigError::*;
        use AuthorizationServerConfigEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateAuthorizationServerConfig {
                authorization_server_config_id,
                grant_types_supported,
                response_types_supported,
                scopes_supported,
                authorization_endpoint,
                token_endpoint,
                jwks_uri,
            } => {
                // if self.authorization_server_config_id.is_empty() {
                Ok(vec![AuthorizationServerConfigCreated {
                    authorization_server_config_id: authorization_server_config_id.clone(),
                    grant_types_supported,
                    response_types_supported,
                    scopes_supported,
                    authorization_endpoint,
                    token_endpoint,
                    jwks_uri,
                }])
                // } else {
                //     Err(AuthorizationServerConfigAlreadyExists)
                // }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use AuthorizationServerConfigEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            AuthorizationServerConfigCreated {
                authorization_server_config_id,
                grant_types_supported,
                response_types_supported,
                scopes_supported,
                authorization_endpoint,
                token_endpoint,
                jwks_uri,
            } => {
                self.authorization_server_config_id = authorization_server_config_id;
                self.grant_types_supported = grant_types_supported;
                self.response_types_supported = response_types_supported;
                self.scopes_supported = scopes_supported;
                self.authorization_endpoint = authorization_endpoint;
                self.token_endpoint = token_endpoint;
                self.jwks_uri = jwks_uri;
            }
        }
    }
}

#[cfg(test)]
pub mod authorization_code_tests {
    use super::test_utils::*;
    use super::*;
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
}
