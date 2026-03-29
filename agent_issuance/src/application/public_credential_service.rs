use agent_shared::{
    convert_iota_jwk_to_decoding_key, get_unverified_jwt_claims, handlers::query_handler,
    serde_json_value_ext::SerdeJsonValueExt,
};
use jsonwebtoken::{decode, decode_header, Validation};
use thiserror::Error;
use tracing::info;

use crate::state::IssuanceState;

pub struct PublicCredentialService {}

impl PublicCredentialService {
    pub async fn get_public_credential(
        &self,
        data_access_consent_token: String,
        state: &IssuanceState,
    ) -> Result<String, PublicCredentialServiceError> {
        // Get unverified claims
        let dact_value = serde_json::Value::String(data_access_consent_token.clone());
        let dact_claims = get_unverified_jwt_claims(&dact_value).ok_or(PublicCredentialServiceError::DACTError(
            "Failed to get JWT claims from Data Access Consent Token".to_string(),
        ))?;

        // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which a Verifier is trying to access
        let sub = dact_claims
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or(PublicCredentialServiceError::DACTError(
                "Failed to get `sub` claim from Data Access Consent Token".to_string(),
            ))?;

        // Because the JTI needs to be a valid URL, we appended the credential ID to the issuer URL of the Public Credential Token.
        // This means only the last segment of the `sub` claim is the actual credential ID.
        let credential_id = sub.rsplit('/').next().ok_or(PublicCredentialServiceError::DACTError(
            "Failed to parse credential ID from `sub` claim in Public Credential Token".to_string(),
        ))?;

        // Get the credential, if there is one for the given `sub`
        let credential = query_handler(credential_id, &state.query.credential)
            .await
            .map_err(|e| {
                PublicCredentialServiceError::QueryError(format!("Failed to get credential from storage: {e}"))
            })?
            .ok_or(PublicCredentialServiceError::CredentialNotFound(
                credential_id.to_string(),
            ))?;

        info!("credential data: {:#?}", credential.signed);
        let signed_credential = credential
            .signed
            .ok_or(PublicCredentialServiceError::CredentialNotFound(
                credential_id.to_string(),
            ))?;
        let credential_claims = get_unverified_jwt_claims(&signed_credential).ok_or(
            PublicCredentialServiceError::InvalidJwtError("Failed to get credential JWT claims".to_string()),
        )?;

        // Extract credential subject ID (we need as_ref to avoid moving credential)
        let credential_subject_id = credential_claims
            .get("vc")
            .and_then(|vc| vc.get("credentialSubject"))
            .and_then(|cs| cs.get("id"))
            .and_then(|id| id.as_str())
            .ok_or(PublicCredentialServiceError::InvalidJwtError(
                "Failed to get credential subject ID from credential claims".to_string(),
            ))?;

        // Extract iss from claims and validate
        // TODO: validate this using the KID
        let iss = dact_claims
            .get("iss")
            .and_then(|v| v.as_str())
            .ok_or(PublicCredentialServiceError::DACTError(
                "Failed to get `iss` claim from Data Access Consent Token".to_string(),
            ))?;

        // Check whether the issuer of the Public Credential Token matches the subject of the requested credential
        // This check means we currently don't allow to publicly share anonymous credentials
        if iss != credential_subject_id {
            return Err(PublicCredentialServiceError::DACTError(
                "Invalid Data Access Consent Token: issuer does not match requested credential subject".to_string(),
            ));
        }

        // TODO: validate status

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

        // Decode header to get kid
        let jwt_header = decode_header(&data_access_consent_token)
            .map_err(|e| PublicCredentialServiceError::InvalidJwtError(format!("Failed to decode JWT header: {e}")))?;

        let kid = jwt_header.kid.ok_or_else(|| {
            PublicCredentialServiceError::InvalidJwtError("JWT header does not contain a kid".to_string())
        })?;

        // Validate the kid belongs to the same DID as credential subject
        // TODO
        // let kid_did = kid.split('#').next().unwrap_or(&kid);
        // if kid_did != credential_subject_id {
        //     return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
        //         .title("Invalid Token")
        //         .message("Public Credential Token kid does not match requested credential subject DID")
        //         .finish());
        // }

        // Fetch the public key using the kid
        let public_key = state.subject.resolve_public_key(&kid).await.map_err(|_| {
            PublicCredentialServiceError::DACTError("Failed to fetch public key for DACT JWT verification".to_string())
        })?;

        let decoding_key =
            convert_iota_jwk_to_decoding_key(&public_key).ok_or(PublicCredentialServiceError::DACTError(
                "Failed to convert public key into decoding key for DACT JWT verification".to_string(),
            ))?;

        let mut validation = Validation::new(jwt_header.alg);
        validation.validate_aud = false; // we are skipping aud validation for now
                                         // TODO: more validation parameters should be set

        // Decode and verify the JWT signature
        decode::<serde_json::Value>(&data_access_consent_token, &decoding_key, &validation)
            .map_err(|e| PublicCredentialServiceError::InvalidJwtError(format!("JWT verification failed: {}", e)))?;

        // Return the credential if all validations pass
        let signed_credential_str =
            signed_credential
                .to_unescaped_string()
                .ok_or(PublicCredentialServiceError::InvalidJwtError(
                    "Failed to convert signed credential to string".to_string(),
                ))?;

        Ok(signed_credential_str)
    }
}

#[derive(Error, Debug)]
pub enum PublicCredentialServiceError {
    #[error("Credential with id {0} not found")]
    CredentialNotFound(String),
    #[error("Error resolving DID: {0}")]
    DidResolutionError(String),
    #[error("Invalid internal credential JWT: {0}")]
    InvalidJwtError(String),
    #[error("Data Access Consent Token error: {0}")]
    DACTError(String),
    #[error("Query error: {0}")]
    QueryError(String),
    // TODO: This error probably is obsolete since validation errors are now handled by the `public_verification_response`.
    #[error("Validation error: {0}")]
    ValidationError(String),
}
