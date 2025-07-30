use crate::{
    domain::{
        access_token::{command::AccessTokenCommand, views::AccessTokenView},
        authorization_code::command::AuthorizationCodeCommand,
    },
    state::{AuthorizationState, UNIME_CLIENT_ID},
};
use agent_issuance::{application::access_token_validation_service::Claims, state::IssuanceState};
use agent_shared::{
    config::{config, get_preferred_did_method, get_preferred_signing_algorithm},
    handlers::{command_handler, query_handler},
};
use jsonwebtoken;
use oid4vc_core::jwt;
use oid4vci::{token_request::TokenRequest, token_response::TokenResponse};
use uuid::Uuid;

pub struct TokenIssuanceService {}

impl TokenIssuanceService {
    pub async fn issue_token(
        authorization_state: &AuthorizationState,
        issuance_state: &IssuanceState,
        token_request: TokenRequest,
        // FIX ME
    ) -> Result<TokenResponse, ()> {
        let (client_id, issuer_state) = match token_request {
            TokenRequest::PreAuthorizedCode {
                pre_authorized_code,
                tx_code: _tx_code,
            } => {
                let issuer_state = query_handler("all_offers", &issuance_state.query.all_offers)
                    .await
                    .expect("FIXME")
                    .expect("FIXME")
                    .offers
                    .into_iter()
                    .find_map(|(offer_id, offer)| {
                        (offer.pre_authorized_code == pre_authorized_code).then_some(offer_id)
                    });

                // FIXME: Replace with actual client ID and issuer state
                (UNIME_CLIENT_ID.to_string(), issuer_state)
            }
            TokenRequest::AuthorizationCode {
                client_id,
                code,
                code_verifier,
                redirect_uri,
            } => {
                let client = query_handler(&client_id, &authorization_state.query.client)
                    .await
                    .expect("FIXME")
                    .expect("FIXME");

                let client_id = client.client_id.clone();

                let command = AuthorizationCodeCommand::RedeemCode {
                    client_id: client_id.clone(),
                    redirect_uri,
                    code_verifier,
                };

                command_handler(&code, &authorization_state.command.authorization_code, command)
                    .await
                    .expect("Failed to handle command");

                let issuer_state = query_handler(&code, &authorization_state.query.authorization_code)
                    .await
                    .expect("FIXME")
                    .expect("FIXME")
                    .issuer_state;

                (client_id, issuer_state)
            }
        };

        let access_token_id = Uuid::new_v4().to_string();

        let access_token_expires_in = 3600; // 1 hour

        let command = AccessTokenCommand::IssueAccessToken {
            access_token_id: access_token_id.clone(),
            user_id: "authenticated_user_id".to_string(), // FIXME: Replace with actual authenticated user ID
            client_id,
            scopes: None, // FIXME: Replace with actual scopes
            access_token_expires_in: access_token_expires_in.clone(),
            refresh_token_expires_in: Some(7200), // 2 hours
            issuer_state,                         // FIXME: Replace with actual issuer state if needed
        };

        command_handler(&access_token_id, &authorization_state.command.access_token, command)
            .await
            .expect("Failed to issue access token");

        let AccessTokenView {
            access_token_id,
            user_id,
            client_id,
            scopes,
            issued_at,
            access_token_expires_at,
            refresh_token_expires_at,
            issuer_state,
        } = query_handler(&access_token_id, &authorization_state.query.access_token)
            .await
            .expect("FIXME")
            .expect("FIXME");

        let claims = Claims {
            iss: config().public_url.to_string(), // FIXME: use DID?
            sub: user_id,
            aud: config().public_url.to_string(),
            exp: access_token_expires_at as u64, // Expiration time in seconds
            iat: issued_at as u64,               // Issued at time in seconds
            jti: access_token_id,
            scopes,
            client_id, // UniMe?
            issuer_state,
        };

        let header = jsonwebtoken::Header::new(get_preferred_signing_algorithm());
        let access_token = jwt::encode(
            authorization_state.signer.clone(),
            header,
            claims,
            &get_preferred_did_method().to_string(),
        )
        .await
        .expect("FIXME: Failed to encode JWT");

        Ok(TokenResponse {
            access_token,
            token_type: "bearer".to_string(), // FIXME: should this be included in the Aggregate?
            expires_in: Some(access_token_expires_in), // 1 hour FIXME: check all the timestamp stuff properly
            scope: None,
            refresh_token: None,
        })
    }
}
