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
                user_id,
                client_id,
                scopes,
                issued_at,
                access_token_expires_at,
                refresh_token_expires_at,
                issuer_state,
            } => {
                self.access_token_id.clone_from(access_token_id);
                self.user_id.clone_from(user_id);
                self.client_id.clone_from(client_id);
                self.scopes.clone_from(scopes);
                self.issued_at = *issued_at;
                self.access_token_expires_at = *access_token_expires_at;
                self.refresh_token_expires_at = *refresh_token_expires_at;
                self.issuer_state.clone_from(issuer_state);
            }
        }
    }
}
