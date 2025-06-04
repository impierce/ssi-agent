pub mod all_tokens;

use super::aggregate::AccessToken;
use cqrs_es::{EventEnvelope, View};

pub type AccessTokenView = AccessToken;

impl View<AccessToken> for AccessToken {
    fn update(&mut self, event: &EventEnvelope<AccessToken>) {
        use super::event::AccessTokenEvent::*;

        match &event.payload {
            AccessTokenIssued {
                access_token_id,
                access_token_value,
                user_id,
                client_id,
                scopes,
                access_token_expires_at,
                refresh_token_expires_at,
                issuer_state,
            } => {
                self.access_token_id.clone_from(access_token_id);
                self.access_token_value.clone_from(access_token_value);
                self.user_id.clone_from(user_id);
                self.client_id.clone_from(client_id);
                self.scopes.clone_from(scopes);
                self.access_token_expires_at = *access_token_expires_at;
                self.refresh_token_expires_at = *refresh_token_expires_at;
                self.issuer_state.clone_from(issuer_state);
            }
        }
    }
}
