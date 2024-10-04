pub mod all_authorization_requests;

use super::aggregate::AuthorizationRequest;
use cqrs_es::{EventEnvelope, View};

pub type AuthorizationRequestView = AuthorizationRequest;

impl View<AuthorizationRequest> for AuthorizationRequest {
    fn update(&mut self, event: &EventEnvelope<AuthorizationRequest>) {
        use crate::authorization_request::event::AuthorizationRequestEvent::*;

        match &event.payload {
            AuthorizationRequestCreated { authorization_request } => {
                self.authorization_request.replace(*authorization_request.clone());
            }
            FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request,
            } => {
                self.form_url_encoded_authorization_request
                    .replace(form_url_encoded_authorization_request.clone());
            }
            AuthorizationRequestObjectSigned {
                signed_authorization_request_object,
            } => {
                self.signed_authorization_request_object
                    .replace(signed_authorization_request_object.clone());
            }
        }
    }
}