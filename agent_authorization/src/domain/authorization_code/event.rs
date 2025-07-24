use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AuthorizationCodeEvent {
    AuthorizationCodeCreated {
        authorization_code_id: String,
        client_id: String,
        redirect_uri: Url,
        scope: Option<String>,
        user_id: String,
        code_challenge: Option<String>,
        code_challenge_method: Option<String>,
        issuer_state: Option<String>,
        expires_at: Option<i64>,
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
