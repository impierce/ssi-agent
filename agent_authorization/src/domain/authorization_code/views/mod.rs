pub mod all_authorization_codes;

use super::aggregate::AuthorizationCode;
use cqrs_es::{EventEnvelope, View};

pub type AuthorizationCodeView = AuthorizationCode;

impl View<AuthorizationCode> for AuthorizationCode {
    fn update(&mut self, event: &EventEnvelope<AuthorizationCode>) {
        use super::event::AuthorizationCodeEvent::*;

        match &event.payload {
            AuthorizationCodeCreated {
                authorization_code_id,
                client_id,
                redirect_uri,
                code_challenge,
                code_challenge_method,
                issuer_state,
                expires_at,
            } => {
                self.authorization_code_id.clone_from(authorization_code_id);
                self.client_id.clone_from(client_id);
                self.redirect_uri.replace(redirect_uri.clone());
                self.code_challenge.clone_from(code_challenge);
                self.code_challenge_method.clone_from(code_challenge_method);
                self.issuer_state.clone_from(issuer_state);
                self.expires_at.replace(expires_at.clone());
            }
            AuthorizationCodeRedeemed {
                authorization_code_id,
                redeemed,
            } => {
                self.authorization_code_id.clone_from(authorization_code_id);
                self.redeemed = *redeemed;
            }
        }
    }
}
