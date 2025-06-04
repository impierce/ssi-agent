use std::str::FromStr;

use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::{authorization_details::AuthorizationDetailsObject, token_response::TokenResponse};
use serde::Serializer;
use uuid::{fmt::Urn, Uuid};

use crate::{
    application::pushed_authorization_service::ClientConfiguration,
    domain::{access_token::command::AccessTokenCommand, authorization_code::command::AuthorizationCodeCommand},
    state::AuthorizationState,
};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: String,
    pub code_verifier: String,
    pub redirect_uri: url::Url,
    pub client_id: String,
}

pub struct TokenIssuanceService {}

impl TokenIssuanceService {
    pub async fn issue_token(
        state: &AuthorizationState,
        token_request: TokenRequest,
        // FIX ME
    ) -> Result<TokenResponse, ()> {
        // FIXME
        let static_unime_configuration = ClientConfiguration {
            client_id: "test_client_id".to_string(),
            redirect_uris: vec![url::Url::parse("unime://callback").expect("Failed to parse URL")],
            grant_types: vec!["authorization_code".to_string()],
            response_types: vec!["code".to_string()],
            token_endpoint_auth_method: "none".to_string(),
            require_pkce: true,
            code_challenge_methods: vec!["S256".to_string()],
            require_par: true,
            client_name: Some("UniMe".to_string()),
            logo_uri: Some("FIXME-logo_uri".to_string()),
            policy_uri: None,
            tos_uri: None,
        };

        let client_id = token_request.client_id.clone();

        let command = AuthorizationCodeCommand::RedeemCode {
            client_id: client_id.clone(),
            redirect_uri: Some(token_request.redirect_uri.to_string()),
            code_verifier: Some(token_request.code_verifier),
        };

        command_handler(&token_request.code, &state.command.authorization_code, command)
            .await
            .expect("Failed to handle command");

        // let access_token_id = Uuid::new_v4().to_string(); // FIXME: Generate a real access token ID
        let access_token_id = Uuid::default().to_string(); // FIXME: Use a real access token ID

        let access_token_expires_in = 3600; // 1 hour

        let issuer_state = query_handler(&token_request.code, &state.query.authorization_code)
            .await
            .expect("FIXME")
            .expect("FIXME")
            .issuer_state;

        let command = AccessTokenCommand::IssueAccessToken {
            access_token_id: access_token_id.clone(),
            user_id: "authenticated_user_id".to_string(), // FIXME: Replace with actual authenticated user ID
            client_id,
            scopes: None, // FIXME: Replace with actual scopes
            access_token_expires_in: access_token_expires_in.clone(),
            refresh_token_expires_in: Some(7200), // 2 hours
            issuer_state,                         // FIXME: Replace with actual issuer state if needed
        };

        command_handler(&access_token_id, &state.command.access_token, command)
            .await
            .expect("Failed to issue access token");

        let access_token_view = query_handler(&access_token_id, &state.query.access_token)
            .await
            .expect("FIXME")
            .expect("FIXME");

        Ok(TokenResponse {
            access_token: access_token_view.access_token_value,
            token_type: "bearer".to_string(), // FIXME: should this be included in the Aggregate?
            expires_in: Some(access_token_expires_in), // 1 hour
            scope: None,
            refresh_token: None,
            // FIXME
            c_nonce: None,
            c_nonce_expires_in: None,
        })
    }
}
