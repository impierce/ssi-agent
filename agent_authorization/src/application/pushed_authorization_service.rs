use std::str::FromStr;

use agent_shared::handlers::command_handler;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use serde::Serializer;
use uuid::{fmt::Urn, Uuid};

use crate::{
    domain::oauth2_authorization_request::command::OAuth2AuthorizationRequestCommand, state::AuthorizationState,
};

// FIXME
// #[skip_serializing_none]
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq)]
pub struct PushedAuthorizationRequest {
    pub response_type: String,
    pub state: String,
    pub client_id: String,
    pub redirect_uri: url::Url,
    pub scope: String,
    #[serde(default)]
    pub client_assertion_type: Option<String>,
    #[serde(default)]
    pub client_assertion: Option<String>,
    pub issuer_state: Option<String>,

    // OID4VCI
    // pub authorization_details: AuthorizationDetailsObject,

    // PKCE
    #[serde(default)]
    pub code_challenge: Option<String>,
    #[serde(default)]
    pub code_challenge_method: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PushedAuthorizationResponse {
    #[serde(serialize_with = "uuid_as_urn")]
    pub request_uri: Uuid,
    pub expires_in: u64,
}

fn uuid_as_urn<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&uuid.urn().to_string())
}

// FIXME
// #[skip_serializing_none]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ClientConfiguration {
    pub client_id: String,
    pub redirect_uris: Vec<url::Url>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: String,
    pub require_pkce: bool,
    pub code_challenge_methods: Vec<String>,
    pub require_par: bool,
    pub client_name: Option<String>,
    pub logo_uri: Option<String>,
    pub policy_uri: Option<url::Url>,
    pub tos_uri: Option<url::Url>,
}

pub struct PushedAuthorizationService {}

impl PushedAuthorizationService {
    pub async fn handle_pushed_authorization_request(
        state: &AuthorizationState,
        pushed_authorization_request: PushedAuthorizationRequest,
        // FIX ME
    ) -> Result<PushedAuthorizationResponse, ()> {
        // FIXME
        let static_unime_configuration = ClientConfiguration {
            client_id: "test_client_id".to_string(),
            redirect_uris: vec![url::Url::parse("https://example.com/callback").expect("Failed to parse URL")],
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

        if pushed_authorization_request.client_id != static_unime_configuration.client_id {
            return Err(());
        }

        if !static_unime_configuration
            .redirect_uris
            .contains(&pushed_authorization_request.redirect_uri)
        {
            return Err(());
        }

        if !static_unime_configuration
            .response_types
            .contains(&pushed_authorization_request.response_type)
        {
            return Err(());
        }

        if pushed_authorization_request.response_type != "code" {
            return Err(());
        }

        if static_unime_configuration.require_pkce || static_unime_configuration.token_endpoint_auth_method == "none" {
            // FIXME: Validate PKCE
        }

        // FIXME
        let request_uri = Uuid::default();
        let oauth2_authorization_request_id = request_uri.urn().to_string();
        let expires_in = 3600; // 1 hour
        let expires_at = chrono::Utc::now().timestamp() + expires_in as i64;

        let command = OAuth2AuthorizationRequestCommand::InitializeFromPushedAuthorizationRequest {
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

        // Here you would implement the logic to handle the Pushed Authorization Request
        // For now, we return a dummy response
        Ok(PushedAuthorizationResponse {
            request_uri,
            expires_in: 3600, // 1 hour
        })
    }
}
