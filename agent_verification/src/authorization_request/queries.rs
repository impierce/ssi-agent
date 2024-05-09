use super::aggregate::AuthorizationRequest;
use crate::generic_oid4vc::GenericAuthorizationRequest;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AuthorizationRequestView {
    pub authorization_request: Option<GenericAuthorizationRequest>,
    pub form_url_encoded_authorization_request: Option<String>,
    pub signed_authorization_request_object: Option<String>,
}

impl View<AuthorizationRequest> for AuthorizationRequestView {
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
