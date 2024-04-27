use oid4vp::oid4vp::OID4VP;
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;

use crate::authorization_request::aggregate::GenericAuthorizationRequest;

// TODO: MOve this somewhere else
pub type SIOPv2AuthorizationResponse = oid4vc_core::authorization_response::AuthorizationResponse<SIOPv2>;
pub type OID4VPAuthorizationResponse = oid4vc_core::authorization_response::AuthorizationResponse<OID4VP>;

// TODO: come up with a better name for this type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum GenericAuthorizationResponse {
    SIOPv2(SIOPv2AuthorizationResponse),
    OID4VP(OID4VPAuthorizationResponse),
}

impl GenericAuthorizationResponse {
    pub fn as_siopv2_authorization_response(&self) -> Option<&SIOPv2AuthorizationResponse> {
        match self {
            GenericAuthorizationResponse::SIOPv2(authorization_response) => Some(authorization_response),
            _ => None,
        }
    }

    pub fn as_oid4vp_authorization_response(&self) -> Option<&OID4VPAuthorizationResponse> {
        match self {
            GenericAuthorizationResponse::OID4VP(authorization_response) => Some(authorization_response),
            _ => None,
        }
    }

    pub fn state(&self) -> Option<&String> {
        match self {
            GenericAuthorizationResponse::SIOPv2(authorization_response) => authorization_response.state.as_ref(),
            GenericAuthorizationResponse::OID4VP(authorization_response) => authorization_response.state.as_ref(),
        }
    }

    pub fn token(&self) -> String {
        match self {
            GenericAuthorizationResponse::SIOPv2(authorization_response) => {
                authorization_response.extension.id_token.clone()
            }
            GenericAuthorizationResponse::OID4VP(authorization_response) => {
                match &authorization_response.extension.oid4vp_parameters {
                    oid4vp::Oid4vpParams::Params { vp_token, .. } => vp_token.clone(),
                    _ => unimplemented!(),
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ConnectionCommand {
    VerifyAuthorizationResponse {
        authorization_request: GenericAuthorizationRequest,
        authorization_response: GenericAuthorizationResponse,
    },
}
