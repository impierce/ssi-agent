use oid4vci::authorization_request::CodeChallengeMethod;
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
        // TODO: implement strong types in `oid4vc` that can be used here.
        grant_types: Vec<String>,
        response_types: Vec<String>,
        token_endpoint_auth_method: String,
        require_pkce: bool,
        code_challenge_methods_supported: Vec<CodeChallengeMethod>,
        require_pushed_authorization_request: bool,
    },
}
