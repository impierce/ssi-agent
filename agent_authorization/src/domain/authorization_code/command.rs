use oid4vci::authorization_request::CodeChallengeMethod;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AuthorizationCodeCommand {
    CreateAuthorizationCode {
        authorization_code_id: String,
        client_id: String,
        redirect_uri: Option<Url>,
        code_challenge: Option<String>,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        expires_in: i64,
    },
    RedeemAuthorizationCode {
        client_id: String,
        redirect_uri: Option<Url>,
        code_verifier: Option<String>,
    },
}

impl CommandOperation for AuthorizationCodeCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateAuthorizationCode { .. } => "authorization.authorization_codes.create",
            Self::RedeemAuthorizationCode { .. } => "authorization.authorization_codes.redeem",
        }
    }
}
