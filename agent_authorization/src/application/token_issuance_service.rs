use std::str::FromStr;

use oid4vci::{
    authorization_details::AuthorizationDetailsObject, token_request::TokenRequest, token_response::TokenResponse,
};
use serde::Serializer;
use uuid::{fmt::Urn, Uuid};

use crate::state::AuthorizationState;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct AuthorizationRequest {
    pub client_id: String,
    #[serde(serialize_with = "uuid_as_urn")]
    pub request_uri: Uuid,
}

fn uuid_as_urn<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&uuid.urn().to_string())
}

pub struct TokenIssuanceService {}

impl TokenIssuanceService {
    pub fn issue_token(
        state: &AuthorizationState,
        token_request: TokenRequest,
        // FIX ME
    ) -> Result<TokenResponse, ()> {
        // Here you would implement the logic to handle the Authorization Request
        // For now, we return a dummy URL
        Ok(TokenResponse {
            access_token: "dummy_access_token".to_string(),
            token_type: "bearer".to_string(),
            expires_in: Some(3600), // 1 hour
            scope: Some("openid".to_string()),
            refresh_token: None,
            // FIXME
            c_nonce: Some("dummy_nonce".to_string()),
            c_nonce_expires_in: Some(3600), // 1 hour
        })
    }
}
