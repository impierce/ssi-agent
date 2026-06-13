use crate::{
    domain::{
        access_token::{command::AccessTokenCommand, views::AccessTokenView},
        authorization_code::command::AuthorizationCodeCommand,
    },
    state::AuthorizationState,
};
use agent_issuance::{application::access_token_validation_service::AccessTokenClaims, state::IssuanceState};
use agent_shared::{
    config::{config, get_preferred_did_method, get_preferred_signing_algorithm},
    handlers::{public_command_handler, public_query_handler},
};
use jsonwebtoken;
use oid4vc_core::jwt;
use oid4vci::{token_request::TokenRequest, token_response::TokenResponse};
use thiserror::Error;
use tracing::warn;

const PLACEHOLDER_CLIENT_ID: &str = "client_id";

#[derive(Debug, Error)]
pub enum TokenIssuanceError {
    #[error("Invalid client ID")]
    InvalidClientIdError,
    #[error("Invalid authorization code: {0}")]
    InvalidAuthorizationCodeError(String),
    #[error("Missing authorization code")]
    MissingAuthorizationCodeError,

    #[error("Transaction code is missing.")]
    MissingTxCodeError,
    #[error("Wrong transaction code provided.")]
    InvalidTxCodeError,
    #[error("TxCode not requested but provided.")]
    UnrequestedTxCodeError,
    #[error("Pre-Authorized Code is invalid.")]
    InvalidPreAuthorizedCodeError,

    #[error("Missing access token")]
    MissingAccessTokenError,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub struct TokenIssuanceService {}
// TODO: Handle authorization_details merging/downscoping logic.
// Per OID4VCI spec Section 6.1 and 6.2  https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-successful-token-response
// - If authorization_details is present in both the Authorization Request
//   (stored in authorization_state) and the Token Request, the Token Request
//   may represent a downscoped subset.
// - The server should validate that the Token Request's authorization_details
//   is a subset of what was authorized in the Authorization Request:
// "[...] it is RECOMMENDED that the AS would accept a request from the Wallet containing a subset of credential_configuration_id parameters
// received in the original Authorization Request and issue a token for the reduced set."
// - The Token Response MUST include authorization_details if it was present
//   in either the Authorization Request or Token Request.
// - Each entry must be enriched with credential_identifiers (within the authorization_details).
//
// For now, we pass through the Token Request's authorization_details as-is.
impl TokenIssuanceService {
    pub async fn issue_token(
        authorization_state: &AuthorizationState,
        issuance_state: &IssuanceState,
        token_request: TokenRequest,
    ) -> Result<TokenResponse, TokenIssuanceError> {
        use TokenIssuanceError::*;

        let (client_id, issuer_state, authorization_details) = match token_request {
            TokenRequest::PreAuthorizedCode {
                pre_authorized_code,
                tx_code,
                authorization_details,
            } => {
                // TODO: make sure that the Pre-Authorized Code is short-lived and single-use.
                // See https://github.com/impierce/ssi-agent/issues/240
                let offer = public_query_handler("all_offers", &issuance_state.query.all_offers)
                    .await
                    .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?
                    .and_then(|all_offers_view| {
                        all_offers_view
                            .offers
                            .into_values()
                            .find_map(|offer| (offer.pre_authorized_code == pre_authorized_code).then_some(offer))
                    })
                    .ok_or(InvalidPreAuthorizedCodeError)?;

                let offer_requires_tx_code = offer.tx_code.is_some();

                match (offer_requires_tx_code, tx_code) {
                    (true, None) => return Err(MissingTxCodeError),
                    (false, Some(_provided_tx_code)) => return Err(UnrequestedTxCodeError),
                    (true, Some(provided_tx_code)) => {
                        let expected_tx_code = offer.tx_code.as_ref().ok_or(MissingTxCodeError)?;

                        if provided_tx_code != *expected_tx_code {
                            return Err(InvalidTxCodeError);
                        }
                    }
                    (false, _) => {}
                }

                let issuer_state = Some(offer.offer_id);

                // TODO: The `client_id` claim in the Pre-Authorized Code request is optional (see https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0-15.html#section-6.1-5),
                // but it is required in the JSON Web Token (JWT) Profile for OAuth 2.0 Access Tokens
                // (see https://datatracker.ietf.org/doc/html/rfc9068#section-2.2-2.10). So as a temporary workaround,
                // we will use a placeholder client ID. This is not ideal, but it is ok for now since the Access Token
                // is opaque to the Client, and it is validated by the Credential Issuer which for now is the same as
                // the Authorization Server.
                warn!("Using placeholder client_id: {}", PLACEHOLDER_CLIENT_ID);
                (PLACEHOLDER_CLIENT_ID.to_string(), issuer_state, authorization_details)
            }
            TokenRequest::AuthorizationCode {
                client_id,
                code,
                code_verifier,
                redirect_uri,
                authorization_details,
            } => {
                let client = public_query_handler(&client_id, &authorization_state.query.client)
                    .await
                    .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?
                    .ok_or(TokenIssuanceError::InvalidClientIdError)?;

                let client_id = client.client_id.clone();

                let command = AuthorizationCodeCommand::RedeemAuthorizationCode {
                    client_id: client_id.clone(),
                    redirect_uri,
                    code_verifier,
                };

                public_command_handler(&code, &authorization_state.command.authorization_code, command)
                    .await
                    .map_err(|err| TokenIssuanceError::InvalidAuthorizationCodeError(err.to_string()))?;

                let issuer_state = public_query_handler(&code, &authorization_state.query.authorization_code)
                    .await
                    .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?
                    // This error should never happen, since we just redeemed the authorization code.
                    .ok_or(TokenIssuanceError::MissingAuthorizationCodeError)?
                    .issuer_state;

                (client_id, issuer_state, authorization_details)
            }
        };

        let access_token_id = uuid::Uuid::new_v4().to_string();

        let access_token_expires_in = 3600; // 1 hour

        let command = AccessTokenCommand::IssueAccessToken {
            access_token_id: access_token_id.clone(),
            // TODO: Since we do not support user authentication yet, we will use a placeholder user ID. When we do
            // support user authentication, for example through SIOPv2, then this value will be the user's DID.
            user_id: "user_id".to_string(),
            client_id,
            // TODO: support scopes
            scopes: None,
            access_token_expires_in,
            // TODO: support refresh tokens
            refresh_token_expires_in: None,
            issuer_state,
        };

        public_command_handler(&access_token_id, &authorization_state.command.access_token, command)
            .await
            .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?;

        let AccessTokenView {
            access_token_id,
            user_id,
            client_id,
            scopes,
            issued_at,
            access_token_expires_at,
            // TODO: support refresh tokens
            refresh_token_expires_at: _refresh_token_expires_at,
            issuer_state,
        } = public_query_handler(&access_token_id, &authorization_state.query.access_token)
            .await
            .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?
            .ok_or(TokenIssuanceError::MissingAccessTokenError)?;

        let claims = AccessTokenClaims {
            // TODO: Could/should this be a DID?
            iss: config().public_url.to_string(),
            sub: user_id,
            // TODO: Could/should this be a DID?
            aud: config().public_url.to_string(),
            exp: access_token_expires_at,
            iat: issued_at,
            jti: access_token_id,
            scope: scopes,
            client_id,
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
        .map_err(|err| TokenIssuanceError::Internal(err.to_string()))?;

        Ok(TokenResponse {
            access_token,
            // TODO: should this be included in the aggregate?
            token_type: "bearer".to_string(),
            expires_in: Some(access_token_expires_in),
            scope: None,
            refresh_token: None,
            // TODO: Ensure that the `credential_identifier` parameter in `authorization_details` is populated correctly before returning.
            // See section 6.2 of the OID4VCI spec: https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-successful-token-response
            authorization_details,
        })
    }
}
