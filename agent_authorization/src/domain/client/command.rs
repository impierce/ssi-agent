use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ClientCommand {
    RegisterClient {
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
