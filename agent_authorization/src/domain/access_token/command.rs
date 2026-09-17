use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum AccessTokenCommand {
    IssueAccessToken {
        access_token_id: String,
        user_id: String,
        client_id: String,
        scopes: Option<String>,
        access_token_expires_in: u64,
        refresh_token_expires_in: Option<u64>,
        issuer_state: Option<String>,
    },
}

impl CommandOperation for AccessTokenCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::IssueAccessToken { .. } => "authorization.access_tokens.issue",
        }
    }
}
