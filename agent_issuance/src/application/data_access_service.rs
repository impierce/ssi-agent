use agent_shared::{
    convert_iota_jwk_to_decoding_key, credential_status_checker::CredentialStatusChecker, get_unverified_jwt_claims,
    handlers::query_handler, serde_json_value_ext::SerdeJsonValueExt,
};
use jsonwebtoken::{decode, decode_header, Validation};
use oauth_tsl::status_list::StatusType;
use oid4vc_core::credential_status_verifier::CredentialStatusVerifier;
use thiserror::Error;
use tracing::info;

use crate::state::IssuanceState;

pub struct DataAccessService {}

impl DataAccessService {
    pub async fn resolve_data_access_consent_token(
        &self,
        data_access_consent_token: String,
        state: &IssuanceState,
    ) -> Result<String, DataAccessServiceError> {
        // Get unverified claims
        let dact_value = serde_json::Value::String(data_access_consent_token.clone());
        let dact_claims = get_unverified_jwt_claims(&dact_value).ok_or(DataAccessServiceError::InvalidDACTError(
            "Failed to get JWT claims from Data Access Consent Token".to_string(),
        ))?;

        // Validate status of Data Access Consent Token
        if let Some(status_claim) = dact_claims.get("status") {
            let credential_status_checker = CredentialStatusChecker {
                verification_material_resolver: state.subject.clone(),
            };

            credential_status_checker
                .check_credential_status(status_claim.to_owned())
                .await
                .map_err(|e| DataAccessServiceError::InvalidDACTError(e.to_string()))?;
        }

        // Validate the signature of the Data Access Consent Token
        let dact_jwt_header = decode_header(&data_access_consent_token).map_err(|e| {
            DataAccessServiceError::InvalidDACTError(format!(
                "Failed to decode JWT header of the Data Access Consent Token: {e}"
            ))
        })?;

        let dact_kid = dact_jwt_header.kid.ok_or(DataAccessServiceError::InvalidDACTError(
            "JWT header is missing `kid` field".to_string(),
        ))?;

        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&dact_kid).await.map_err(|_| {
            DataAccessServiceError::InvalidDACTError("Failed to fetch public key for JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(DataAccessServiceError::InvalidDACTError(
                "Failed to convert public key into decoding key for JWT verification".to_string(),
            ))?;

        // TODO: should more validation parameters be set??
        let mut validation = Validation::new(dact_jwt_header.alg);
        validation.validate_aud = false;

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&data_access_consent_token, &decoding_key, &validation).map_err(|e| {
            DataAccessServiceError::InvalidDACTError(format!(
                "JWT signature verification failed for the Data Access Consent Token: {e}"
            ))
        })?;

        // TODO: check if the aud claim is the same as the issuer of the requested credential and of this instance
        // TODO: skip `aud` validation for now
        // // Extract the `aud` claim
        // let aud = claims.get("aud").and_then(|v| v.as_str()).ok_or_else(|| {
        //     ApiError::builder(StatusCode::BAD_REQUEST)
        //         .title("Invalid Token")
        //         .message("Failed to get `aud` claim from Public Credential Token")
        //         .finish()
        // })?;

        // // Validate that the `aud` matches an enabled DID of this Unicorn instance
        // let supported_signing_algorithms = get_all_enabled_signing_algorithms_supported();
        // let enabled_did_methods = get_all_enabled_did_methods();

        // let mut dids = Vec::new();

        // // TODO: this is ugly but for now the easiest way for me to get all did_methods for the hackathon
        // // In fact i don't need the full identifier (kid), just the DID.
        // for did_method in &enabled_did_methods {
        //     for alg in &supported_signing_algorithms {
        //         let did = state
        //             .subject
        //             .identifier(did_method.to_string().as_ref(), *alg)
        //             .await
        //             .map_err(|_| ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR).finish())?;

        //         dids.push(
        //             did.split('#')
        //                 .next()
        //                 .ok_or(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR))?
        //                 .to_string(),
        //         );
        //     }
        // }

        // if !dids.contains(&aud.to_string()) {
        //     return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
        //         .title("Invalid Token")
        //         .message("Public Credential Token audience does not match this holder's DID")
        //         .finish());
        // }

        // The subject of the Data Access Consent Token is the JTI (credential ID) of the requested credential.
        let sub = dact_claims
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessServiceError::InvalidDACTError(
                "Failed to get `sub` claim from Data Access Consent Token".to_string(),
            ))?;

        // Get the credential, if there is one for the given `sub`
        let credential = query_handler(sub, &state.query.credential)
            .await
            .map_err(|e| {
                DataAccessServiceError::QueryError(format!("Failed to get requested credential from storage: {e}"))
            })?
            .ok_or(DataAccessServiceError::CredentialNotFound(sub.to_string()))?;

        info!("Requested credential data: {:#?}", credential.signed);

        let signed_credential = credential
            .signed
            .ok_or(DataAccessServiceError::CredentialNotFound(sub.to_string()))?;
        // TODO: perhaps use the credential.data.raw field here instead of get_unverified_jwt_claims as this only works with jwt, not sd-jwt I believe. But I'm not sure if the jwt claims are added to data.raw
        let credential_claims = get_unverified_jwt_claims(&signed_credential).ok_or(
            DataAccessServiceError::InvalidRequestedCredentialError("Failed to get credential JWT claims".to_string()),
        )?;

        // Extract credential subject ID (we need as_ref to avoid moving credential)
        let credential_subject_id = credential_claims
            .get("vc")
            .and_then(|vc| vc.get("credentialSubject"))
            .and_then(|cs| cs.get("id"))
            .and_then(|id| id.as_str())
            .ok_or(DataAccessServiceError::InvalidRequestedCredentialError(
                "Failed to get credential subject ID from credential claims".to_string(),
            ))?;

        // Check whether the issuer of the Data Access Consent Token matches the subject of the requested credential
        // This check means we currently don't allow to publicly share anonymous credentials
        let dact_did = dact_kid.split('#').next().unwrap_or(&dact_kid);

        if dact_did != credential_subject_id {
            return Err(DataAccessServiceError::InvalidDACTError(
                "Invalid Data Access Consent Token: issuer does not match requested credential subject".to_string(),
            ));
        }

        // Validate the status of the requested credential
        if credential.credential_status.status != StatusType::VALID {
            return Err(DataAccessServiceError::InvalidRequestedCredentialError(format!(
                "Credential with id {sub} is not valid according to its credential status"
            )));
        }

        // TODO check the exp for the dact everywhere

        // Return the credential if all validations pass
        let signed_credential_str =
            signed_credential
                .to_unescaped_string()
                .ok_or(DataAccessServiceError::InvalidRequestedCredentialError(
                    "Failed to convert signed credential to string".to_string(),
                ))?;

        info!("Successfully resolved Data Access Consent Token, returning requested credential");

        Ok(signed_credential_str)
    }
}

#[derive(Error, Debug)]
pub enum DataAccessServiceError {
    #[error("Credential with id {0} not found")]
    CredentialNotFound(String),
    #[error("Error resolving DID: {0}")]
    DidResolutionError(String),
    #[error("Invalid internal Requested Credential: {0}")]
    InvalidRequestedCredentialError(String),
    #[error("Invalid Data Access Consent Token: {0}")]
    InvalidDACTError(String),
    #[error("Query error: {0}")]
    QueryError(String),
}
