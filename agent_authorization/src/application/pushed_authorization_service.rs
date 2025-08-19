use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::authorization_request::AuthorizationRequest;
use oid4vci::wallet::PushedAuthorizationResponse;
use uuid::Uuid;

use crate::{
    domain::oauth2_authorization_request::command::OAuth2AuthorizationRequestCommand, state::AuthorizationState,
};

pub struct PushedAuthorizationService {}

impl PushedAuthorizationService {
    pub async fn handle_pushed_authorization_request(
        state: &AuthorizationState,
        pushed_authorization_request: AuthorizationRequest,
        // FIX ME
    ) -> Result<PushedAuthorizationResponse, ()> {
        tracing::info!("client id: {}", pushed_authorization_request.client_id);
        let client = query_handler(&pushed_authorization_request.client_id, &state.query.client)
            .await
            .expect("FIXME")
            .expect("FIXME");

        if !client
            .redirect_uris
            .contains(pushed_authorization_request.redirect_uri.as_ref().expect("FIXME"))
        {
            return Err(());
        }

        if !client
            .response_types
            .contains(&pushed_authorization_request.response_type)
        {
            return Err(());
        }

        // FIXME: add scope validation if needed

        // FIXME: add errors
        if client.require_pkce {
            if pushed_authorization_request.code_challenge.is_none() {
                return Err(());
            }

            if let Some(code_challenge_method) = pushed_authorization_request.code_challenge_method.as_ref() {
                if !client.code_challenge_methods_supported.contains(code_challenge_method) {
                    return Err(());
                }
            } else {
                return Err(());
            }
        }

        // FIXME
        let request_uri = Uuid::new_v4();
        let oauth2_authorization_request_id = request_uri.to_string();
        let expires_in = 3600; // 1 hour
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let command = OAuth2AuthorizationRequestCommand::CreateOAuth2AuthorizationRequest {
            oauth2_authorization_request_id: oauth2_authorization_request_id.clone(),
            pushed_authorization_request: pushed_authorization_request.clone(),
            expires_at,
        };

        command_handler(
            &oauth2_authorization_request_id,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .expect("Failed to handle command");

        Ok(PushedAuthorizationResponse {
            request_uri,
            expires_in,
        })
    }
}
