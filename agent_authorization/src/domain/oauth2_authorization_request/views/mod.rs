pub mod all_oauth2_authorization_requests;

use super::aggregate::OAuth2AuthorizationRequest;
use cqrs_es::{EventEnvelope, View};

pub type OAuth2AuthorizationRequestView = OAuth2AuthorizationRequest;

impl View<OAuth2AuthorizationRequest> for OAuth2AuthorizationRequest {
    fn update(&mut self, event: &EventEnvelope<OAuth2AuthorizationRequest>) {
        use super::event::OAuth2AuthorizationRequestEvent::*;

        match &event.payload {
            AuthorizationRequestPushed {
                oauth2_authorization_request_id,
                response_type,
                state,
                client_id,
                redirect_uri,
                scope,
                client_assertion_type,
                client_assertion,
                issuer_state,
                code_challenge,
                code_challenge_method,

                expires_at,
            } => {
                self.oauth2_authorization_request_id
                    .clone_from(oauth2_authorization_request_id);
                self.response_type.clone_from(response_type);
                self.state.clone_from(state);
                self.client_id.clone_from(client_id);
                self.redirect_uri.clone_from(redirect_uri);
                self.scope.clone_from(scope);
                self.client_assertion_type.clone_from(client_assertion_type);
                self.client_assertion.clone_from(client_assertion);
                self.issuer_state.clone_from(issuer_state);
                self.code_challenge.clone_from(code_challenge);
                self.code_challenge_method.clone_from(code_challenge_method);
                self.expires_at = *expires_at;
            }
        }
    }
}
