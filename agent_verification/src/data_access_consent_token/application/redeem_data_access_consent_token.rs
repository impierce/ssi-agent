use agent_shared::handlers::query_handler;

use crate::{data_access_consent_token::error::DataAccessConsentTokenError, state::VerificationState};
use agent_shared::get_unverified_jwt_claims;

pub struct RedeemDataAccessConsentTokenService {}

impl RedeemDataAccessConsentTokenService {
    pub async fn redeem_data_access_consent_token(
        self,
        token_id: String,
        state: &VerificationState,
    ) -> Result<(), DataAccessConsentTokenError> {
        // 1. query token (domain layer)
        // 2. validate token (domain layer)
        // 3. send token to issuer (http layer)
        // 4. validate response (classic vp logic)
        // 5. return credential 
        // Data Access Consent Token will hereafter be referred to as DACT for brevity
        let data_access_consent_token = query_handler(&token_id, &state.query.data_access_consent_token)
            .await
            .map_err(|e| DataAccessConsentTokenError::QueryError(e.to_string()))?
            .ok_or(DataAccessConsentTokenError::DataAccessConsentTokenNotFound(token_id.clone()))?;

        // Initialize response to "invalid" default, if a check passes the response is updated accordingly
        let mut public_verification_response = PublicVerificationResponse::default();

        // Get unverified claims
        let token_value = serde_json::Value::String(data_access_consent_token.token.clone());
        let dact_claims = get_unverified_jwt_claims(&token_value).ok_or(DataAccessConsentTokenError::JwtDecodingError("Failed to get the unverified JWT claims".to_string()))?;

        // Extract the `aud` claim, it equals the issuer DID of the credential which is given access to.
        let aud = dact_claims
            .get("aud")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessConsentTokenError::JwtDecodingError("Failed to get `aud` claim from DACT".to_string()))?;

        let resolver = state.subject.resolver;
        let issuer_did_document = resolver.resolve(aud).await.map_err(|e| DataAccessConsentTokenError::DidResolutionError(e.to_string()))?;

        // Check and validate domain linkage

        info!("Issuer DID Document: {:#?}", issuer_did_document);

        let mut linked_domains = get_issuer_linked_domains(&issuer_did_document).await;
        for url in linked_domains.clone() {
            let validation_result = validate_domain_linkage(&resolver, url.clone(), aud).await;
            if validation_result.status == ValidationStatus::Success {
                public_verification_response.domain_linkage = ValidationResult {
                    status: ValidationStatus::Success,
                    payload: Some(url.to_string()),
                    data: None,
                };
                break;
            }
        }

    // Fallback for did:webs if no domain linkage is found
    if linked_domains.is_empty() || aud.starts_with("did:web") {
        let did_web_domain = extract_url_from_did_web(aud).ok_or(
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to resolve issuer DID")
                .finish(),
        )?;
        info!("Extracted URL from did:web: {:#?}", did_web_domain);
        public_verification_response.domain_linkage = ValidationResult {
            status: ValidationStatus::Success,
            payload: Some(did_web_domain.to_string()),
            data: None,
        };
        linked_domains.push(did_web_domain);
    }

    info!("Linked Domains: {:#?}", linked_domains);

    // Validate the issuers linked verifiable presentations and then check if any of them were issued to this verifier to establish a trust relation.

    // Get this instance's DID's
    // TODO: this is ugly but for now the easiest way for me to get all did_methods for the hackathon
    let supported_signing_algorithms = get_all_enabled_signing_algorithms_supported();
    let enabled_did_methods = get_all_enabled_did_methods();

    let mut dids = Vec::new();

    // TODO: In fact i don't need the full identifier (kid), just the DID.
    for did_method in &enabled_did_methods {
        for alg in &supported_signing_algorithms {
            let did = state
                .subject
                .identifier(did_method.to_string().as_ref(), *alg)
                .await
                .map_err(|_| ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR).finish())?;

            dids.push(
                did.split('#')
                    .next()
                    .ok_or(ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR))?
                    .to_string(),
            );
        }
    }

    info!("DIDs to match against: {:#?}", dids);
    let linked_verifiable_credentials: Vec<_> =
        validate_linked_verifiable_presentations(&resolver, &issuer_did_document)
            .await
            .into_iter()
            .flatten()
            .filter(|linked_verifiable_credential| {
                info!(
                    "Validating linked verifiable credential: {:#?}",
                    linked_verifiable_credential
                );
                // Check if the issuer of the linked verifiable credential matches the DID of this verifier to establish a trust relation
                let claims = match get_unverified_jwt_claims(&linked_verifiable_credential.data) {
                    Ok(claims) => claims,
                    Err(_) => return false,
                };

                info!("Linked VC claims: {:#?}", claims);
                info!("DIDs to match against: {:#?}", dids);

                claims
                    .get("iss")
                    .and_then(|iss| iss.as_str())
                    .and_then(|iss| match dids.contains(&iss.to_string()) {
                        true => Some(true),
                        false => None,
                    })
                    .unwrap_or(false)
            })
            .collect();

    if !linked_verifiable_credentials.is_empty() {
        // TODO: this is hardcoded logic for the hackathon demo
        let data = get_unverified_jwt_claims(&linked_verifiable_credentials[0].data).map_err(|_| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Linked Verifiable Presentation")
                .message("Failed to get the credential data from the linked verifiable presentation")
                .finish()
        })?["vc"]
            .clone();

        public_verification_response.linked_vp.status = ValidationStatus::Success;
        public_verification_response.linked_vp.data = Some(data);
        public_verification_response.trust_relation.status = ValidationStatus::Success;
    } else {
        public_verification_response.linked_vp = ValidationResult {
            status: ValidationStatus::Failure,
            // TODO: this is a hackathon specific message
            payload: Some("No valid certifications found for the issuer".to_string()),
            data: None,
        };
        public_verification_response.trust_relation = ValidationResult {
            status: ValidationStatus::Failure,
            // TODO: this is a hackathon specific message
            payload: Some("Trust relation between this verifier and the issuer could not be established".to_string()),
            data: None,
        };
    }

    // TODO: validate status of Public Credential Token
    // Invalid = BAD_REQUEST

    // All primary checks have passed for the Public Credential Token at this point, to perform the remaining checks we need to fetch the Public Credential from the Issuer.

    // Discover public credential endpoint through DID resolution
    let public_credential_endpoint = issuer_did_document
        .service()
        .iter()
        .find(|service| service.type_().contains("PublicCredentialEndpoint"))
        .and_then(|service| match service.service_endpoint() {
            ServiceEndpoint::One(url) => Some(url.clone()),
            // TODO: handle multiple endpoints?
            ServiceEndpoint::Set(urls) => urls.first().cloned(),
            ServiceEndpoint::Map(map) => map.values().next().and_then(|urls| urls.first().cloned()),
        })
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Public Credential Endpoint Not Found")
                .message("Issuer DID Document is missing PublicCredentialEndpoint service")
                .finish()
        })?;
        
        let public_credential_endpoint_url_with_parameter =
            format!("{}?public-credential-token={}", public_credential_endpoint, jwt);
    
        Ok(())
    }
}

// TODO:  separate fetching (axum) and validation logic

use std::sync::Arc;

use agent_holder::credential::aggregate::get_unverified_jwt_claims;
use agent_secret_manager::subject::get_public_key_from_kid;
use agent_shared::config::{get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported};
use crate::state::VerificationState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use did_manager::Resolver;
use identity_iota::document::{verifiable, ServiceEndpoint};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tracing::info;
use url::Url;

use crate::v0::{
    issuance::credential_issuer::credential,
    verification::{
        validate_domain_linkage::{get_issuer_linked_domains, validate_domain_linkage, ValidationStatus},
        validate_linked_verifiable_presentation::validate_linked_verifiable_presentations,
    },
};

#[derive(Serialize, Default)]
pub struct ValidationResult {
    status: ValidationStatus,
    payload: Option<String>,
    data: Option<serde_json::Value>,
}

// TODO: make stronger typing then strings
#[derive(Serialize, Default)]
pub struct PublicVerificationResponse {
    pub credential: Option<serde_json::Value>,
    pub proof: ValidationResult,
    pub status: ValidationResult,
    pub trust_relation: ValidationResult,
    pub linked_vp: ValidationResult,
    pub domain_linkage: ValidationResult,
}

/// This endpoint receives a Public Credential Token as a query parameter and then performs several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response along with the validation results.
/// When any validation fails, only the validation results are returned.
/// Both the Verifier and the Issuer need to perform all these checks on the Public Credential Token, zero trust is assumed.
pub async fn public_verification(
    State(state): State<Arc<VerificationState>>,
    Query(parameter): Query<PublicVerificationQuery>,
) -> Result<Response, ApiError> {


    // Fetch Public Credential from issuer endpoint
    let response = reqwest::get(public_credential_endpoint_url_with_parameter)
        .await
        .map_err(|e| {
            ApiError::builder(StatusCode::BAD_GATEWAY)
                .title("Failed to Fetch Public Credential")
                .message(format!(
                    "Failed to get response from Issuer Public Credential endpoint: {e}"
                ))
                .finish()
        })?;

    let verifiable_credential = response.json::<serde_json::Value>().await.map_err(|e| {
        ApiError::builder(StatusCode::BAD_GATEWAY)
            .title("Invalid Public Credential Response")
            .message(format!("Failed to parse Issuer Public Credential response: {e}"))
            .finish()
    })?;

    // Validate all remaining checks

    // Get unverified claims
    let verifiable_credential_claims = get_unverified_jwt_claims(&verifiable_credential).map_err(|_| {
        ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Invalid JWT")
            .message("Failed to decode Public Credential")
            .finish()
    })?;

    // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which a Verifier is trying to access
    let sub = public_credential_token_claims
        .get("sub")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Token")
                .message("Failed to get `sub` claim from Public Credential Token")
                .finish()
        })?;

    let jti = verifiable_credential_claims
        .get("jti")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ApiError::builder(StatusCode::BAD_REQUEST)
                .title("Invalid Credential")
                .message("Failed to get `jti` claim from Public Credential")
                .finish()
        })?;

    if sub != jti {
        return Err(ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
            .title("Invalid Token")
            .message("Public Credential Token `sub` claim does not match Public Credential `jti` claim")
            .finish());
    }

    // TODO: none of this works with sd-jwt yet

    // Extract credential subject ID
    // let credential_subject_id = verifiable_credential_claims.get("vc")
    //     .and_then(|data| data.get("credentialSubject"))
    //     .and_then(|cred_subject| cred_subject.get("id"))
    //     .and_then(|id| id.as_str())
    //     .ok_or_else(|| {
    //         ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
    //             .title("Invalid Credential")
    //             .message("Requested credential is missing the credentialSubject.id field. Publicly sharing anonymous credentials is not supported.")
    //             .finish()
    //     })?;

    // Extract iss from claims and validate
    // let iss = public_credential_token_claims
    //     .get("iss")
    //     .and_then(|v| v.as_str())
    //     .ok_or_else(|| {
    //         ApiError::builder(StatusCode::BAD_REQUEST)
    //             .title("Invalid Token")
    //             .message("Failed to get `iss` claim from Public Credential Token")
    //             .finish()
    //     })?;

    // Check whether the issuer of the Public Credential Token matches the subject of the requested credential
    // This check means we currently don't allow to publicly share anonymous credentials
    // if iss != credential_subject_id {
    //     return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
    //         .title("Invalid Token")
    //         .message("Public Credential Token issuer does not match requested credential subject")
    //         .finish());
    // }

    // TODO: validate status
    public_verification_response.status.status = ValidationStatus::Success;

    // let verifiable_credential_str = verifiable_credential
    //     .as_str()
    //     .ok_or_else(|| {
    //         ApiError::builder(StatusCode::BAD_REQUEST)
    //             .title("Invalid Credential")
    //             .message("Public Credential is not a valid JWT")
    //             .finish()
    //     })?;

    // // Decode header to get kid
    // let jwt_header = decode_header(&verifiable_credential_str).map_err(|e| {
    //     ApiError::builder(StatusCode::BAD_REQUEST)
    //         .title("Invalid Token")
    //         .message(format!("Failed to decode Public Credential Token header: {e}"))
    //         .finish()
    // })?;

    // let kid = jwt_header.kid.ok_or_else(|| {
    //     ApiError::builder(StatusCode::BAD_REQUEST)
    //         .title("Invalid Token")
    //         .message("Failed to get `kid` from Public Credential Token header")
    //         .finish()
    // })?;

    // Validate the kid belongs to the same DID as credential subject
    // let kid_did = kid.split('#').next().unwrap_or(&kid);
    // if kid_did != credential_subject_id {
    //     return Err(ApiError::builder(StatusCode::UNAUTHORIZED)
    //         .title("Invalid Token")
    //         .message("Public Credential Token kid does not match requested credential subject DID")
    //         .finish());
    // }

    // // Fetch the public key using the kid
    // let public_key = get_public_key_from_kid(&kid).await.map_err(|_| {
    //     ApiError::builder(StatusCode::BAD_REQUEST)
    //         .title("Invalid Token")
    //         .message("Failed to retrieve public key for kid")
    //         .finish()
    // })?;

    // // TODO: more validation parameters should be set
    // let validation = Validation::new(jwt_header.alg);
    // validation.set_issuer(&[credential_subject_id]);
    // validation.set_audience(&[aud]);
    // validation.sub = Some(sub.to_string());

    // Decode and verify the JWT signature
    // let _token_data = decode::<serde_json::Value>(&jwt, &decoding_key, &validation).map_err(|e| {
    //     ApiError::builder(StatusCode::BAD_REQUEST)
    //         .title("Invalid Token")
    //         .message(format!("JWT verification failed: {}", e))
    //         .finish()
    // })?;

    public_verification_response.proof.status = ValidationStatus::Success;

    // If all validations have passed, set the credential in the response
    if public_verification_response.proof.status == ValidationStatus::Success
        && public_verification_response.status.status == ValidationStatus::Success
        && public_verification_response.trust_relation.status == ValidationStatus::Success
        && public_verification_response.linked_vp.status == ValidationStatus::Success
        && public_verification_response.domain_linkage.status == ValidationStatus::Success
    {
        let credential_data = verifiable_credential_claims.get("vc").cloned().ok_or_else(|| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Invalid Public Credential received")
                .message("Public Credential data could not be extracted from the received response")
                .finish()
        })?;
        public_verification_response.credential = Some(credential_data);
    }

    // Return the credential if all validations pass
    Ok((StatusCode::OK, Json(public_verification_response)).into_response())
}

// Helpers

fn extract_url_from_did_web(did_web: &str) -> Option<Url> {
    if let Some(did) = did_web.strip_prefix("did:web:") {
        let url_str = if let Some(index_colon) = did.find(':') {
            &did[..index_colon]
        } else {
            did
        };

        // TODO: quick hack to solve the percent-encoding issue in did:web:localhost%3A3033 (localhost:3033)
        let url_decoded = url_str.replace("%3A", ":");

        if let Ok(url) = Url::parse(&format!("https://{url_decoded}")) {
            return Some(url);
        }
    }
    None
}
