pub mod all_oauth2_authorization_requests;

use super::aggregate::OAuth2AuthorizationRequest;
use cqrs_es::{EventEnvelope, View};

pub type OAuth2AuthorizationRequestView = OAuth2AuthorizationRequest;

impl View<OAuth2AuthorizationRequest> for OAuth2AuthorizationRequest {
    fn update(&mut self, event: &EventEnvelope<OAuth2AuthorizationRequest>) {
        use super::event::OAuth2AuthorizationRequestEvent::*;

        match &event.payload {
            OAuth2AuthorizationRequestCreated {
                oauth2_authorization_request_id,
                response_type,
                state,
                client_id,
                redirect_uri,
                scope,
                issuer_state,
                authorization_details,
                code_challenge,
                code_challenge_method,

                expires_at,

                openid4vp_request,
            } => {
                self.oauth2_authorization_request_id
                    .clone_from(oauth2_authorization_request_id);
                self.response_type.clone_from(response_type);
                self.state.clone_from(state);
                self.client_id.clone_from(client_id);
                self.redirect_uri.clone_from(redirect_uri);
                self.scope.clone_from(scope);
                self.issuer_state.clone_from(issuer_state);
                self.authorization_details.clone_from(authorization_details);
                self.code_challenge.clone_from(code_challenge);
                self.code_challenge_method.clone_from(code_challenge_method);
                self.expires_at = *expires_at;
                self.openid4vp_request.clone_from(openid4vp_request);
            }
            OAuth2AuthorizationRequestExpired {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id
                    .clone_from(oauth2_authorization_request_id);
                self.consent_status = consent_status.clone();
            }
            ConsentGranted {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id
                    .clone_from(oauth2_authorization_request_id);
                self.consent_status = consent_status.clone();
            }
            ConsentRejected {
                oauth2_authorization_request_id,
                consent_status,
            } => {
                self.oauth2_authorization_request_id
                    .clone_from(oauth2_authorization_request_id);
                self.consent_status = consent_status.clone();
            }
        }
    }
}
