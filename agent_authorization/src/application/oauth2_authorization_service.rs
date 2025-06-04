use std::str::FromStr;

use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::authorization_details::AuthorizationDetailsObject;
use reqwest::redirect;
use serde::Serializer;
use uuid::{fmt::Urn, Uuid};

use crate::{domain::authorization_code::command::AuthorizationCodeCommand, state::AuthorizationState};

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
            &authorization_request.request_uri.urn().to_string(),
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

        let redirect_uri = oauth2_authorization_request.redirect_uri.clone();

        // let authorization_code_id = uuid::Uuid::new_v4().to_string();
        let authorization_code_id = uuid::Uuid::default().to_string(); // FIXME
        let command = AuthorizationCodeCommand::CreateAuthorizationCode {
            authorization_code_id: authorization_code_id.clone(),
            client_id: authorization_request.client_id,
            redirect_uri: oauth2_authorization_request.redirect_uri,
            scope: None, // FIXME: This should be replaced with the actual scope
            user_id: "authenticated_user_id".to_string(), // FIXME: This should be replaced with the actual authenticated user ID
            // authorization_details: AuthorizationDetailsObject::default(), // FIXME: This should be replaced with the actual authorization details
            code_challenge: oauth2_authorization_request.code_challenge,
            code_challenge_method: oauth2_authorization_request.code_challenge_method,
            issuer_state: oauth2_authorization_request.issuer_state,
            expires_in: Some(600), // 10 minutes
        };

        command_handler(&authorization_code_id, &state.command.authorization_code, command)
            .await
            .expect("FIXME");

        // FIXME: Add `state` and other necessary parameters to the URL
        Ok(format!("{redirect_uri}?code={authorization_code_id}")
            .parse::<url::Url>()
            .expect("FIXME: Failed to parse URL")
            .to_string())
    }
}
