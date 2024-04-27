use cqrs_es::{EventEnvelope, View};
use oid4vc_core::authorization_request::Object;
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;

use super::aggregate::Connection;

pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ConnectionView {
    id_token: Option<String>,
    vp_token: Option<String>,
}

impl View<Connection> for ConnectionView {
    fn update(&mut self, event: &EventEnvelope<Connection>) {
        use crate::connection::event::ConnectionEvent::*;

        match &event.payload {
            SIOPv2AuthorizationResponseVerified { id_token } => {
                self.id_token.replace(id_token.clone());
            }
            OID4VPAuthorizationResponseVerified { vp_token } => {
                self.vp_token.replace(vp_token.clone());
            }
        }
    }
}
