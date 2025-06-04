use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum AccessTokenEvent {
    AccessTokenIssued {
        access_token_id: String,
        access_token_value: String, // FIXME: use `JWT`
        user_id: String,
        client_id: String,
        scopes: Option<String>,
        access_token_expires_at: u64,
        refresh_token_expires_at: Option<u64>,
        issuer_state: Option<String>,
    },
}

impl DomainEvent for AccessTokenEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
