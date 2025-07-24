use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::wallet::AuthorizationRequestByReference;
use uuid::Uuid;

use crate::{
    domain::{
        authorization_code::command::AuthorizationCodeCommand,
        client,
        oauth2_authorization_request::{
            aggregate::{ConsentStatus, OAuth2AuthorizationRequest},
            command::OAuth2AuthorizationRequestCommand,
        },
    },
    state::AuthorizationState,
};

pub enum ConsentServiceResponse {
    Found(String),
    // FIXME: what if rejected?
}

pub struct ConsentService {}

impl ConsentService {
    pub async fn handle_consent(
        state: &AuthorizationState,
        client_id: String,
        request_uri: Uuid,
        consent_given: bool,
        // FIX ME
    ) -> Result<ConsentServiceResponse, ()> {
        // FIXME: query first
        let command = if consent_given {
            OAuth2AuthorizationRequestCommand::GrantConsent
        } else {
            OAuth2AuthorizationRequestCommand::RejectConsent
        };

        // FIXME: check client_id
        println!("Request URI here: {}", request_uri);

        let oauth_authorization_request = request_uri.to_string();

        command_handler(
            &oauth_authorization_request,
            &state.command.oauth2_authorization_request,
            command,
        )
        .await
        .expect("FIXME");

        // FIXME: do the encoding differently
        let request_uri = request_uri.urn().to_string();
        let encoded_request_uri = urlencoding::encode(&request_uri).clone();

        Ok(ConsentServiceResponse::Found(format!(
            "/auth/authorize?client_id={client_id}&request_uri={encoded_request_uri}",
        )))
    }
}
