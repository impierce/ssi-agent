use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::{authorization_details::AuthorizationDetailsObject, wallet::AuthorizationRequestByReference};
use serde::{Deserialize, Serialize};
use thiserror::Error;
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConsentPageViewModel {
    pub client_id: String,
    pub client_name: String,
    pub scope: String,
    pub authorization_details: Vec<AuthorizationDetailsObject>,
    #[serde()]
    pub request_uri: Uuid,
}

#[derive(Debug, Error)]
pub enum ConsentQueryError {
    #[error("Request not found or expired")]
    RequestNotFound,
    #[error("Client not found")]
    ClientNotFound,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct ConsentQueryService {}

impl ConsentQueryService {
    pub async fn prepare_consent_page_data(
        state: &AuthorizationState,
        request_uri: Uuid,
        // FIX ME
    ) -> Result<ConsentPageViewModel, ConsentQueryError> {
        let authorization_request = query_handler(&request_uri.to_string(), &state.query.oauth2_authorization_request)
            .await
            .expect("FIXME")
            .expect("FIXME");

        let client = query_handler(&authorization_request.client_id, &state.query.client)
            .await
            .expect("FIXME")
            .expect("FIXME");

        Ok(ConsentPageViewModel {
            client_id: client.client_id.clone(),
            client_name: client.client_name.unwrap_or(client.client_id),
            scope: authorization_request.scope,
            authorization_details: authorization_request.authorization_details,
            request_uri: request_uri,
        })
    }
}
