use crate::{
    domain::{
        authorization_code::command::AuthorizationCodeCommand, oauth2_authorization_request::aggregate::ConsentStatus,
    },
    state::AuthorizationState,
};
use agent_shared::handlers::{command_handler, query_handler};
use oid4vci::wallet::uuid_as_urn;
use oid4vci::{to_form_urlencoded_string, wallet::AuthorizationRequestByReference};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

// FIXME: rename this and reuse in consent.rs
#[derive(Deserialize, Serialize)]
pub struct GetLoginQuery {
    #[serde(serialize_with = "uuid_as_urn")]
    pub request_uri: Uuid,
}

pub enum OAuth2AuthorizationServiceResponse {
    RedirectToConsent(String),
    RedirectToClient(Url),
}

pub struct OAuth2AuthorizationService {}

impl OAuth2AuthorizationService {
    pub async fn handle_authorization_request(
        state: &AuthorizationState,
        AuthorizationRequestByReference { client_id, request_uri }: AuthorizationRequestByReference,
        // FIX ME
    ) -> Result<OAuth2AuthorizationServiceResponse, ()> {
        let oauth2_authorization_request = query_handler(
            request_uri.to_string().as_ref(),
            &state.query.oauth2_authorization_request,
        )
        .await
        .expect("FIXME")
        .expect("FIXME");

        if chrono::Utc::now().timestamp() > oauth2_authorization_request.expires_at {
            return Err(());
        }

        println!("{client_id} :  {}", oauth2_authorization_request.client_id);

        if client_id != oauth2_authorization_request.client_id {
            return Err(());
        }

        let client = query_handler(&oauth2_authorization_request.client_id, &state.query.client)
            .await
            .expect("FIXME")
            .expect("FIXME");

        match oauth2_authorization_request.consent_status {
            ConsentStatus::Pending => {
                let get_login_query = GetLoginQuery { request_uri };

                let encoded =
                    to_form_urlencoded_string(&get_login_query).expect("FIXME: Failed to encode authorization request");

                Ok(OAuth2AuthorizationServiceResponse::RedirectToConsent(format!(
                    "/auth/consent?{encoded}"
                )))
            }
            ConsentStatus::Given => {
                let redirect_uri = oauth2_authorization_request.redirect_uri.expect("FIXME");

                let authorization_code_id = uuid::Uuid::new_v4().to_string();
                let command = AuthorizationCodeCommand::CreateAuthorizationCode {
                    authorization_code_id: authorization_code_id.clone(),
                    client_id,
                    redirect_uri: redirect_uri.clone(),
                    scope: None, // FIXME: This should be replaced with the actual scope
                    user_id: "authenticated_user_id".to_string(), // FIXME: This should be replaced with the actual authenticated user ID
                    // authorization_details: AuthorizationDetailsObject::default(), // FIXME: This should be replaced with the actual authorization details
                    code_challenge: oauth2_authorization_request.code_challenge,
                    code_challenge_method: oauth2_authorization_request.code_challenge_method,
                    issuer_state: oauth2_authorization_request.issuer_state,
                    expires_in: 600, // 10 minutes
                };

                command_handler(&authorization_code_id, &state.command.authorization_code, command)
                    .await
                    .expect("FIXME");

                // FIXME: Add `state` and other necessary parameters to the URL
                let redirect_uri_with_code = redirect_uri
                    .join(&format!("?code={authorization_code_id}"))
                    .expect("FIXME: Failed to join redirect URI");

                Ok(OAuth2AuthorizationServiceResponse::RedirectToClient(
                    redirect_uri_with_code,
                ))
            }
            ConsentStatus::Rejected => {
                return Err(());
            }
        }
    }
}
