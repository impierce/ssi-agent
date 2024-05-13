use oid4vc_core::authorization_request::{Body, Object};
use oid4vp::oid4vp::OID4VP;
use serde::{Deserialize, Serialize};
use siopv2::siopv2::SIOPv2;

// TODO(oid4vc): All types and functionalities in this file should be implemented properly in the `oid4vc` crates.

pub type SIOPv2AuthorizationResponse = oid4vc_core::authorization_response::AuthorizationResponse<SIOPv2>;
pub type OID4VPAuthorizationResponse = oid4vc_core::authorization_response::AuthorizationResponse<OID4VP>;
pub type SIOPv2AuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<SIOPv2>>;
pub type OID4VPAuthorizationRequest = oid4vc_core::authorization_request::AuthorizationRequest<Object<OID4VP>>;

/// This enum serves as an abstraction over the different types of authorization responses UniCore can provide
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
}

#[cfg(test)]
impl GenericAuthorizationResponse {
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum GenericAuthorizationRequest {
    SIOPv2(Box<SIOPv2AuthorizationRequest>),
    OID4VP(Box<OID4VPAuthorizationRequest>),
}

impl GenericAuthorizationRequest {
    pub fn as_siopv2_authorization_request(&self) -> Option<&SIOPv2AuthorizationRequest> {
        match self {
            GenericAuthorizationRequest::SIOPv2(authorization_request) => Some(authorization_request),
            _ => None,
        }
    }

    pub fn as_oid4vp_authorization_request(&self) -> Option<&OID4VPAuthorizationRequest> {
        match self {
            GenericAuthorizationRequest::OID4VP(authorization_request) => Some(authorization_request),
            _ => None,
        }
    }

    pub fn client_id(&self) -> String {
        match self {
            GenericAuthorizationRequest::SIOPv2(authorization_request) => {
                authorization_request.body.client_id().clone()
            }
            GenericAuthorizationRequest::OID4VP(authorization_request) => {
                authorization_request.body.client_id().clone()
            }
        }
    }
}
