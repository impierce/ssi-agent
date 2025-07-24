use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationCodeCommand {
    CreateAuthorizationCode {
        authorization_code_id: String,
        client_id: String,
        user_id: String,
        redirect_uri: Url,
        scope: Option<String>,
        code_challenge: Option<String>,
        code_challenge_method: Option<String>,
        issuer_state: Option<String>,
        expires_in: Option<u64>,
    },
    RedeemCode {
        client_id: String,
        redirect_uri: Option<Url>,
        code_verifier: Option<String>,
    },
}
