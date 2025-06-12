use crate::domain::{
    credential::{command::CredentialCommand, views::CredentialView},
    offer::{command::OfferCommand, views::OfferView},
    server_config::views::ServerConfigView,
};
use crate::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_shared::{
    config::config,
    handlers::{command_handler, query_handler},
};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use oid4vci::{credential_request::CredentialRequest, credential_response::CredentialResponse};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::error;

// This struct represents the claims your Issuance BC expects to find in the JWT.
#[derive(Debug, Deserialize)]
pub struct AccessTokenClaims {
    pub iss: String, // Issuer
    pub sub: String, // Subject (user_id)
    pub aud: String, // Audience
    // ... other standard claims like exp, iat, jti ...
    pub issuer_state: String, // Custom claim linking to the Credential Offer
}

/// A service to handle application-level logic for credential issuance.
pub struct CredentialIssuanceService;

impl CredentialIssuanceService {
    /// Handles the entire credential issuance flow for a given request.
    pub async fn issue_credential(
        state: &IssuanceState,
        access_token: String,
        credential_request: CredentialRequest,
    ) -> Result<CredentialResponse, ()> {
        // First, validate the token and extract the offer_id from its claims.
        let (offer_id, credential_issuer_metadata, authorization_server_metadata) =
            Self::validate_token_and_get_context(state, &access_token)
                .await
                .unwrap();

        // Verify the proof within the credential request against the offer.
        let verify_command = OfferCommand::VerifyCredentialRequest {
            offer_id: offer_id.clone(),
            credential_issuer_metadata: Box::new(credential_issuer_metadata.credential_issuer_metadata),
            // authorization_server_metadata: Box::new(credential_issuer_metadata.authorization_server_metadata),
            credential_request,
        };
        command_handler(&offer_id, &state.command.offer, verify_command)
            .await
            .unwrap();

        // Poll for the credential IDs and subject_id from the OfferView.
        // This part waits for an external system to provide necessary data.
        let (credential_ids, subject_id) = Self::await_credential_data(state, &offer_id).await.unwrap();

        // Sign all the credentials associated with the offer.
        let mut signed_credentials = Vec::new();
        for credential_id in credential_ids {
            let signed_credential_tuple = Self::sign_credential(state, &credential_id, &subject_id).await.unwrap();
            signed_credentials.push(signed_credential_tuple);
        }

        // Create the final credential response.
        let response_command = OfferCommand::CreateCredentialResponse {
            offer_id: offer_id.clone(),
            signed_credentials,
        };
        command_handler(&offer_id, &state.command.offer, response_command)
            .await
            .unwrap();

        // Query the view one last time to get the created credential response.
        Ok(query_handler(&offer_id, &state.query.offer)
            .await
            .unwrap()
            .and_then(|offer_view| offer_view.credential_response)
            .unwrap())
        // .ok_or_else(internal_server_error)
    }

    /// Validates the JWT access token and returns the offer_id (`issuer_state`) and metadata.
    async fn validate_token_and_get_context(
        state: &IssuanceState,
        access_token: &str,
    ) -> Result<(String, Box<ServerConfigView>, Box<ServerConfigView>), ()> {
        let (credential_issuer_metadata, authorization_server_metadata) =
            match query_handler(SERVER_CONFIG_ID, &state.query.server_config)
                .await
                .unwrap()
            {
                Some(view) => (Box::new(view.clone()), Box::new(view.clone())),
                // _ => return Err(internal_server_error()),
                _ => return Err(todo!()),
            };

        let header = decode_header(access_token).unwrap();
        let kid = header.kid.unwrap();
        let alg = header.alg;

        todo!("FIXME");
        // let jwks_uri = authorization_server_metadata
        //     .jwks_uri
        //     .as_ref()
        //     // .ok_or_else(internal_server_error)?;
        //     .ok_or_else(todo!())?;
        let jwks_uri = "FIXME";
        let jwks: jsonwebtoken::jwk::JwkSet = reqwest::get(jwks_uri)
            .await
            // .map_err(|_| internal_server_error())?
            .map_err(|_| todo!())?
            .json()
            .await
            // .map_err(|_| internal_server_error())?;
            .map_err(|_| todo!())?;

        let jwk = jwks.find(&kid).unwrap();
        let decoding_key = DecodingKey::from_jwk(jwk).unwrap();

        let mut validation = Validation::new(alg);
        // validation.set_issuer(&[authorization_server_metadata.issuer.as_str()]);
        // validation.set_audience(&[credential_issuer_metadata.credential_issuer.as_str()]);
        validation.validate_exp = true;

        let decoded_token = decode::<AccessTokenClaims>(access_token, &decoding_key, &validation).unwrap();

        let offer_id = decoded_token.claims.issuer_state;
        Ok((offer_id, credential_issuer_metadata, authorization_server_metadata))
    }

    /// Waits for the credential data to be available in the OfferView.
    async fn await_credential_data(state: &IssuanceState, offer_id: &str) -> Result<(Vec<String>, String), ()> {
        let timeout = config().external_server_response_timeout_ms;
        let start_time = Instant::now();

        loop {
            match query_handler(offer_id, &state.query.offer).await.unwrap() {
                Some(OfferView { credential_ids, .. }) if credential_ids.is_empty() => {
                    if start_time.elapsed().as_millis() <= timeout.into() {
                        sleep(Duration::from_millis(100)).await; // Polling interval
                    } else {
                        error!("Timeout waiting for credential data for offer_id: {}", offer_id);
                        // return Err(internal_server_error());
                        return Err(todo!());
                    }
                }
                Some(OfferView {
                    credential_ids,
                    subject_id: Some(subject_id),
                    ..
                }) => {
                    return Ok((credential_ids, subject_id));
                }
                // _ => return Err(internal_server_error()),
                _ => return Err(todo!()),
            }
        }
    }

    /// Dispatches the command to sign a single credential and queries for the result.
    async fn sign_credential(
        state: &IssuanceState,
        credential_id: &str,
        subject_id: &str,
    ) -> Result<(serde_json::Value, Option<String>), ()> {
        let command = CredentialCommand::SignCredential {
            credential_id: credential_id.to_string(),
            subject_id: subject_id.to_string(),
            overwrite: false,
        };
        command_handler(credential_id, &state.command.credential, command)
            .await
            .unwrap();

        match query_handler(credential_id, &state.query.credential).await.unwrap() {
            Some(CredentialView {
                signed: Some(signed_credential),
                notification_id,
                ..
            }) => Ok((signed_credential, notification_id)),
            // _ => Err(internal_server_error()),
            _ => Err(todo!()),
        }
    }
}
