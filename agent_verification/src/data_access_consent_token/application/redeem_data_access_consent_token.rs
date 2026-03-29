use crate::{
    data_access_consent_token::{
        application::{
            validate_domain_linkage::{get_issuer_linked_domains, validate_domain_linkage, ValidationStatus},
            validate_linked_verifiable_presentation::validate_linked_verifiable_presentations,
        },
        error::DataAccessConsentTokenError,
    },
    state::VerificationState,
};

use agent_shared::{
    config::{get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported},
    get_unverified_jwt_claims,
    handlers::query_handler,
};
use oid4vc_core::Subject;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use identity_iota::{document::{ServiceEndpoint, verifiable}, iota_interaction::types::object::Data};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use url::Url;

pub const DATA_ACCESS_ENDPOINT: &str = "DataAccessConsentTokenEndpoint";
pub struct RedeemDataAccessConsentTokenService {}

impl RedeemDataAccessConsentTokenService {
    pub async fn validate_data_access_consent_token(
        self,
        token_id: String,
        state: &VerificationState,
    ) -> Result<(Url, String), DataAccessConsentTokenError> {
        // 1. query token (domain layer)
        // 2. validate token (domain layer)
        // 3. send token to issuer (http layer)
        // 4. validate response (classic vp logic)
        // 5. return credential
        // Data Access Consent Token will hereafter be referred to as DACT for brevity
        let data_access_consent_token = query_handler(&token_id, &state.query.data_access_consent_token)
            .await
            .map_err(|e| DataAccessConsentTokenError::QueryError(e.to_string()))?
            .ok_or(DataAccessConsentTokenError::DataAccessConsentTokenNotFound(
                token_id.clone(),
            ))?;

        // Initialize response to "invalid" default, if a check passes the response is updated accordingly
        let mut public_verification_response = PublicVerificationResponse::default();

        // Get unverified claims
        let token_value = serde_json::Value::String(data_access_consent_token.token.clone());
        let dact_claims = get_unverified_jwt_claims(&token_value).ok_or(
            DataAccessConsentTokenError::JwtDecodingError("Failed to get the unverified JWT claims".to_string()),
        )?;

        // Extract the `aud` claim, it equals the issuer DID of the credential which is given access to.
        let aud =
            dact_claims
                .get("aud")
                .and_then(|v| v.as_str())
                .ok_or(DataAccessConsentTokenError::JwtDecodingError(
                    "Failed to get `aud` claim from DACT".to_string(),
                ))?;

        let resolver = &state.subject.resolver;
        let issuer_did_document = resolver
            .resolve(aud)
            .await
            .map_err(|e| DataAccessConsentTokenError::DidResolutionError(e.to_string()))?;

        // Check and validate domain linkage

        info!("Issuer DID Document: {:#?}", issuer_did_document);

        let mut linked_domains = get_issuer_linked_domains(&issuer_did_document).await;
        for url in linked_domains.clone() {
            let validation_result = validate_domain_linkage(&resolver, url.clone(), aud).await;
            if validation_result.status == ValidationStatus::Success {
                public_verification_response.domain_linkage.push(ValidationResult {
                    status: ValidationStatus::Success,
                    payload: Some(url.to_string()),
                    data: None,
                });
            } else {
                linked_domains.retain(|u| u != &url);
            }
        }

        // Fallback for did:webs if no domain linkage is found
        if linked_domains.is_empty() {
            match aud.starts_with("did:web") {
                true => {
                    let did_web_domain =
                        extract_url_from_did_web(aud).ok_or(DataAccessConsentTokenError::DidResolutionError(
                            "Failed to extract URL from Issuer did:web".to_string(),
                        ))?;

                    info!("Extracted URL from did:web: {:#?}", did_web_domain);
                    public_verification_response.domain_linkage.push(ValidationResult {
                        status: ValidationStatus::Success,
                        payload: Some(did_web_domain.to_string()),
                        data: None,
                    });
                    linked_domains.push(did_web_domain);
                }
                false => {
                    public_verification_response.domain_linkage.push(ValidationResult {
                        status: ValidationStatus::Failure,
                        payload: Some("No linked domains found for issuer, and issuer is not a did:web".to_string()),
                        data: None,
                    });
                }
            }
        }

        info!("Linked Domains: {:#?}", linked_domains);

        // Get and validate the issuers linked verifiable presentations.
        let linked_verifiable_credentials: Vec<_> =
            validate_linked_verifiable_presentations(&resolver, &issuer_did_document)
                .await
                .into_iter()
                .flatten()
                .collect();

        match linked_verifiable_credentials.is_empty() {
            true => {
                public_verification_response.linked_vp.push(ValidationResult {
                    status: ValidationStatus::Failure,
                    // TODO: this is a hackathon specific message
                    payload: Some("No valid certifications found for the issuer".to_string()),
                    data: None,
                });
            }
            false => {
                for linked_vp in &linked_verifiable_credentials {
                    public_verification_response.linked_vp.push(ValidationResult {
                        status: ValidationStatus::Success,
                        // TODO: this is a hackathon specific message
                        payload: Some("Valid certifications found for the issuer".to_string()),
                        data: Some(serde_json::to_value(linked_vp).map_err(|_e| {
                            DataAccessConsentTokenError::DidResolutionError(
                                "TODO: this is an incorrect error message".to_string(),
                            )
                        })?), // TODO
                    });
                }
            }
        }

        info!("Linked Verifiable Credentials: {:#?}", linked_verifiable_credentials);

        // TODO: validate status of Public Credential Token
        // TODO: validate the trust relation.

        // TODO: All primary checks have passed for the Public Credential Token at this point, to perform the remaining checks we need to fetch the Public Credential from the Issuer.

        // Discover public credential endpoint through DID resolution
        let data_access_endpoint = issuer_did_document
            .service()
            .iter()
            .find(|service| service.type_().contains(DATA_ACCESS_ENDPOINT))
            .and_then(|service| match service.service_endpoint() {
                ServiceEndpoint::One(url) => Some(url.clone()),
                // TODO: handle multiple endpoints?
                ServiceEndpoint::Set(urls) => urls.first().cloned(),
                ServiceEndpoint::Map(map) => map.values().next().and_then(|urls| urls.first().cloned()),
            })
            .ok_or(DataAccessConsentTokenError::NoDataAccessEndpointFound(
                "No Data Access Endpoint found in the Issuer DID Document services".to_string(),
            ))?;

        let data_access_endpoint_url = Url::parse(&data_access_endpoint.to_string())
            .map_err(|_| DataAccessConsentTokenError::NoDataAccessEndpointFound(
                "Failed to parse Data Access Endpoint into URL".to_string(),
            ))?;

        Ok((data_access_endpoint_url, data_access_consent_token.token))
    }

    pub async fn validate_data_access_endpoint_response(
        self,
        data_access_consent_token: String,
        response: DataAccessEndpointResponse,
    ) -> Result<(), DataAccessConsentTokenError> {
        let verifiable_credential_claims = get_unverified_jwt_claims(&serde_json::Value::String(response.verifiable_credential)).ok_or(DataAccessConsentTokenError::JwtDecodingError("Failed to get JWT claims".to_string()))?;

        // The subject of the Public Credential Token is the JTI (credential ID) of the issued credential which the Verifier is trying to access
        let sub = verifiable_credential_claims
            .get("sub")
            .and_then(|v| v.as_str())
            .ok_or(DataAccessConsentTokenError::JwtDecodingError("Failed to get `sub` claim from Public Credential Token".to_string()))?;

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
        
        Ok(())
    }
}

// TODO: should we enable access tokens for multiple credentials? then this should be a vec
pub struct DataAccessEndpointResponse {
    pub verifiable_credential: String,
}

// TODO: review if we still want these response structs or not
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
    pub linked_vp: Vec<ValidationResult>,
    pub domain_linkage: Vec<ValidationResult>,
}

/// This endpoint receives a Public Credential Token as a query parameter and then performs several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response along with the validation results.
/// When any validation fails, only the validation results are returned.
/// Both the Verifier and the Issuer need to perform all these checks on the Public Credential Token, zero trust is assumed.
pub async fn public_verification(
    State(state): State<Arc<VerificationState>>,
    Query(parameter): Query<PublicVerificationQuery>,
) -> Result<Response, ApiError> {


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
