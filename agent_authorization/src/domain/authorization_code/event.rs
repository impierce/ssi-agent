use cqrs_es::DomainEvent;
use oid4vci::authorization_request::CodeChallengeMethod;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AuthorizationCodeEvent {
    AuthorizationCodeCreated {
        authorization_code_id: String,
        client_id: String,
        redirect_uri: Option<Url>,
        code_challenge: Option<String>,
        code_challenge_method: Option<CodeChallengeMethod>,
        issuer_state: Option<String>,
        expires_at: i64,
    },
    AuthorizationCodeRedeemed {
        authorization_code_id: String,
        redeemed: bool,
    },
}

impl DomainEvent for AuthorizationCodeEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
