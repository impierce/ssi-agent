use std::str::FromStr;

use agent_shared::handlers::query_handler;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use serde::Serializer;
use uuid::{fmt::Urn, Uuid};

use crate::state::AuthorizationState;

// FIXME: Only PAR?
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

pub struct OAuth2AuthorizationService {}

impl OAuth2AuthorizationService {
    pub async fn handle_authorization_request(
        state: &AuthorizationState,
        authorization_request: AuthorizationRequest,
        // FIX ME
    ) -> Result<String, ()> {
        let oauth2_authorization_request = query_handler(
            &authorization_request.request_uri.to_string(),
            &state.query.oauth2_authorization_request,
        )
        .await
        .expect("FIXME")
        .expect("FIXME");

        if chrono::Utc::now().timestamp() > oauth2_authorization_request.expires_at {
            return Err(());
        }

        if oauth2_authorization_request.client_id != authorization_request.client_id {
            return Err(());
        }

        // Here you would implement the logic to handle the Authorization Request
        // For now, we return a dummy URL
        Ok("unime://example?code=code"
            .parse::<url::Url>()
            .expect("Failed to parse URL")
            .to_string())
    }
}
