use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationServerConfigCommand {
    CreateAuthorizationServerConfig {
        authorization_server_config_id: String,
        grant_types_supported: Vec<String>,
        response_types_supported: Vec<String>,
        scopes_supported: Vec<String>,
        authorization_endpoint: String,
        token_endpoint: String,
        jwks_uri: Option<String>,
    },
}
