use cqrs_es::{EventEnvelope, View};
use oid4vc_core::authorization_request::Object;
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;

use super::aggregate::{AuthorizationRequest, GenericAuthorizationRequest};

pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;

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
                self.authorization_request = Some(*authorization_request.clone());
            }
            FormUrlEncodedAuthorizationRequestCreated {
                form_url_encoded_authorization_request,
            } => {
                self.form_url_encoded_authorization_request = Some(form_url_encoded_authorization_request.clone());
            }
            AuthorizationRequestObjectSigned {
                signed_authorization_request_object,
            } => {
                self.signed_authorization_request_object = Some(signed_authorization_request_object.clone());
            }
        }
    }
}
